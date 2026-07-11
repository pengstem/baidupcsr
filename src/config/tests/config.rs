use std::path::{Path, PathBuf};

use super::super::*;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name)
}

fn write_ui_config(name: &str, data: &str) -> Result<PathBuf, ConfigError> {
    let path = std::env::temp_dir().join(format!(
        "baidupcsr-ui-config-{name}-{}.toml",
        std::process::id()
    ));
    std::fs::write(&path, data).map_err(|source| ConfigError::Write {
        path: path.clone(),
        source,
    })?;
    Ok(path)
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

    let mut expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?)
        .map_err(|source| ConfigError::Parse {
            path: path.clone(),
            source,
        })?;
    expected["future_root_option"] = serde_json::json!({ "enabled": true });
    expected["baidu_user_list"][0]["future_account_option"] = serde_json::json!(["preserve", 7]);
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&expected).map_err(|source| ConfigError::Parse {
            path: path.clone(),
            source,
        })?,
    )
    .map_err(|source| ConfigError::Write {
        path: path.clone(),
        source,
    })?;

    save_active_user_workdir(&path, "/电影/科幻电影")?;
    expected["baidu_user_list"][0]["workdir"] = serde_json::json!("/电影/科幻电影");
    let actual: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?)
        .map_err(|source| ConfigError::Parse {
            path: path.clone(),
            source,
        })?;

    assert_eq!(actual, expected);
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
fn ui_config_parses_standard_toml_table_comments_quotes_whitespace_and_field_order(
) -> Result<(), ConfigError> {
    let path = write_ui_config(
        "standard-toml",
        r#"
            # Fields deliberately do not follow the struct order.
            [video_preview]
            max_chunks = 3
            profile = "isolated" # quoted enum value
            enabled = false
            chunk_size_bytes = 131072
        "#,
    )?;

    let config = load_ui_config_or_default(&path)?;

    assert_eq!(
        config,
        UiConfig {
            video_preview: VideoPreviewConfig {
                enabled: false,
                chunk_size_bytes: 131_072,
                max_chunks: 3,
                player_profile: VideoPlayerProfile::Isolated,
            },
        }
    );
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn ui_config_accepts_explicit_user_profile() -> Result<(), ConfigError> {
    let path = write_ui_config("user-profile", "[video_preview]\nprofile = \"user\"\n")?;

    let config = load_ui_config_or_default(&path)?;

    assert_eq!(
        config.video_preview.player_profile,
        VideoPlayerProfile::User
    );
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn ui_config_rejects_unsupported_video_profile() -> Result<(), ConfigError> {
    let path = write_ui_config(
        "invalid-profile",
        "[video_preview]\nprofile = \"portable\"\n",
    )?;

    let result = load_ui_config_or_default(&path);

    assert!(matches!(
        result,
        Err(ConfigError::InvalidUiConfig { message, .. })
            if message.contains("user") && message.contains("isolated")
    ));
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn ui_config_ignores_unknown_top_level_and_video_keys() -> Result<(), ConfigError> {
    let path = write_ui_config(
        "unknown-keys",
        r#"
            future_top_level = "retained by its owner"

            [video_preview]
            max_chunks = 5
            future_video_option = { mode = "adaptive" }
        "#,
    )?;

    let config = load_ui_config_or_default(&path)?;

    assert_eq!(config.video_preview.max_chunks, 5);
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn ui_config_missing_fields_use_individual_defaults() -> Result<(), ConfigError> {
    let path = write_ui_config("partial-defaults", "[video_preview]\nmax_chunks = 7\n")?;

    let config = load_ui_config_or_default(&path)?;
    let defaults = UiConfig::default();

    assert_eq!(
        config,
        UiConfig {
            video_preview: VideoPreviewConfig {
                max_chunks: 7,
                ..defaults.video_preview
            },
        }
    );
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn ui_config_reports_invalid_ui_config_for_malformed_toml() -> Result<(), ConfigError> {
    let path = write_ui_config("malformed", "[video_preview\nenabled = true\n")?;

    let result = load_ui_config_or_default(&path);

    assert!(matches!(
        result,
        Err(ConfigError::InvalidUiConfig { path: error_path, .. }) if error_path == path
    ));
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn ui_config_rejects_zero_video_chunk_size() -> Result<(), ConfigError> {
    let path = write_ui_config("zero-chunk-size", "[video_preview]\nchunk_size_bytes = 0\n")?;

    let result = load_ui_config_or_default(&path);

    assert!(matches!(
        result,
        Err(ConfigError::InvalidUiConfig { message, .. })
            if message.contains("greater than zero")
    ));
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn ui_config_rejects_zero_video_max_chunks() -> Result<(), ConfigError> {
    let path = write_ui_config("zero-max-chunks", "[video_preview]\nmax_chunks = 0\n")?;

    let result = load_ui_config_or_default(&path);

    assert!(matches!(
        result,
        Err(ConfigError::InvalidUiConfig { message, .. })
            if message.contains("greater than zero")
    ));
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn ui_config_missing_file_uses_defaults() -> Result<(), ConfigError> {
    let path = std::env::temp_dir().join(format!(
        "baidupcsr-ui-config-missing-{}.toml",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    let config = load_ui_config_or_default(&path)?;

    assert_eq!(config, UiConfig::default());
    Ok(())
}
