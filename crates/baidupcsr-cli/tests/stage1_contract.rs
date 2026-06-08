use std::process::Command;

#[test]
fn doctor_redact_reports_stage1_shell() {
    let missing_config_dir = std::env::temp_dir()
        .join(format!("baidupcsr-missing-config-{}", std::process::id()))
        .display()
        .to_string();
    let output_result = Command::new(env!("CARGO_BIN_EXE_baidupcsr-cli"))
        .args(["doctor", "--redact", "--config-dir", &missing_config_dir])
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
    assert!(stdout.contains("baidupcsr doctor"));
    assert!(stdout.contains("redacted: true"));
    assert!(stdout.contains("config: not loaded"));
}
