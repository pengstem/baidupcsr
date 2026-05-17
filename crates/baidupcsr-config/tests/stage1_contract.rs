use baidupcsr_config::crate_name;

#[test]
fn config_crate_exposes_identity() {
    assert_eq!(crate_name(), "baidupcsr-config");
}
