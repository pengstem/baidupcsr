use baidupcsr_config::UiConfig;

#[test]
fn default_ui_config_enables_strict_video_after_explicit_preview_key() {
    let config = UiConfig::default();

    assert!(config.video_preview.enabled);
    assert_eq!(config.video_preview.chunk_size_bytes, 256 * 1024);
    assert_eq!(config.video_preview.max_chunks, 4);
}
