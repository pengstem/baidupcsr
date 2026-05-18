use std::path::Path;
use std::process::Command;

fn fixture_dir() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config/doctor_minimal")
        .display()
        .to_string()
}

#[test]
fn doctor_reports_release_readiness_paths_without_secrets() -> Result<(), Box<dyn std::error::Error>>
{
    let output = Command::new(env!("CARGO_BIN_EXE_baidupcsr-cli"))
        .args(["doctor", "--redact", "--config-dir", &fixture_dir()])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;

    assert!(stdout.contains("ui_config_path:"));
    assert!(stdout.contains("cache_dir:"));
    assert!(stdout.contains("video_preview: enabled"));
    assert!(stdout.contains("mpv:"));
    assert!(stdout.contains("config_permission:"));
    assert!(!stdout.contains("FAKE_BDUSS"));
    assert!(!stdout.contains("FAKE_STOKEN"));

    Ok(())
}

#[test]
fn doctor_gives_repair_hint_when_config_is_missing() -> Result<(), Box<dyn std::error::Error>> {
    let missing = std::env::temp_dir().join(format!(
        "baidupcsr-stage10-missing-config-{}",
        std::process::id()
    ));
    if missing.exists() {
        std::fs::remove_dir_all(&missing)?;
    }
    std::fs::create_dir_all(&missing)?;

    let output = Command::new(env!("CARGO_BIN_EXE_baidupcsr-cli"))
        .args([
            "doctor",
            "--redact",
            "--config-dir",
            missing.to_string_lossy().as_ref(),
        ])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;

    assert!(stdout.contains("config: not loaded"));
    assert!(stdout.contains("repair: set BAIDUPCS_GO_CONFIG_DIR"));

    std::fs::remove_dir_all(&missing)?;
    Ok(())
}
