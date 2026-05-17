use std::process::Command;

#[test]
fn doctor_redact_reports_stage1_shell() {
    let output_result = Command::new(env!("CARGO_BIN_EXE_baidupcsr-cli"))
        .args(["doctor", "--redact"])
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
    assert!(stdout.contains("baidupcsr doctor"));
    assert!(stdout.contains("redacted: true"));
    assert!(stdout.contains("config: not loaded"));
}
