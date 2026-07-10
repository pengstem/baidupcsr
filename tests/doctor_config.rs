use std::path::Path;
use std::process::Command;

#[test]
fn doctor_redact_reads_explicit_config_dir_without_secrets() {
    let fixture_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config/doctor_minimal");
    let output_result = Command::new(env!("CARGO_BIN_EXE_baidupcsr-cli"))
        .args([
            "doctor",
            "--redact",
            "--config-dir",
            fixture_dir.to_string_lossy().as_ref(),
        ])
        .output();
    assert!(output_result.is_ok(), "failed to run baidupcsr-cli doctor");
    let Ok(output) = output_result else {
        return;
    };

    assert!(output.status.success());
    let stdout_result = String::from_utf8(output.stdout);
    assert!(stdout_result.is_ok(), "stdout is not utf8");
    let Ok(stdout) = stdout_result else {
        return;
    };

    assert!(stdout.contains("config: loaded"));
    assert!(stdout.contains("active_uid: 100000001"));
    assert!(stdout.contains("workdir: /"));
    assert!(stdout.contains("bduss: present"));
    assert!(stdout.contains("stoken: present"));
    assert!(!stdout.contains("FAKE_BDUSS"));
    assert!(!stdout.contains("FAKE_STOKEN"));
}
