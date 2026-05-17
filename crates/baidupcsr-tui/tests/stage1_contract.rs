use baidupcsr_tui::crate_name;

#[test]
fn tui_crate_exposes_identity() {
    assert_eq!(crate_name(), "baidupcsr-tui");
}
