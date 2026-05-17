use baidupcsr_core::crate_name;

#[test]
fn core_crate_exposes_identity() {
    assert_eq!(crate_name(), "baidupcsr-core");
}
