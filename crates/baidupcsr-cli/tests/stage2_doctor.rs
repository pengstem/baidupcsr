use std::path::Path;
use std::process::Command;

fn fixture_dir() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config/doctor_minimal")
        .display()
        .to_string()
}

#[test]
fn doctor_redact_reads_explicit_config_dir_without_secrets() {
    let output_result = Command::new(env!("CARGO_BIN_EXE_baidupcsr-cli"))
        .args(["doctor", "--redact", "--config-dir", &fixture_dir()])
        .output();
    assert!(output_result.is_ok(), "failed to run baidupcsr-cli doctor");
    let output = match output_result {
        Ok(output) => output,
        Err(_) => return,
    };

    assert!(output.status.success());
    let stdout_result = String::from_utf8(output.stdout);
    assert!(stdout_result.is_ok(), "stdout is not utf8");
    let stdout = match stdout_result {
        Ok(stdout) => stdout,
        Err(_) => return,
    };

    assert!(stdout.contains("config: loaded"));
    assert!(stdout.contains("active_uid: 100000001"));
    assert!(stdout.contains("workdir: /"));
    assert!(stdout.contains("bduss: present"));
    assert!(stdout.contains("stoken: present"));
    assert!(!stdout.contains("FAKE_BDUSS"));
    assert!(!stdout.contains("FAKE_STOKEN"));
}
