use baidupcsr_pcs::crate_name;

#[test]
fn pcs_crate_exposes_identity() {
    assert_eq!(crate_name(), "baidupcsr-pcs");
}
