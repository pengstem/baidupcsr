use baidupcsr_preview::crate_name;

#[test]
fn preview_crate_exposes_identity() {
    assert_eq!(crate_name(), "baidupcsr-preview");
}
