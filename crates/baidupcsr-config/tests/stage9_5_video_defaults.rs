use baidupcsr_config::{UiConfig, VideoPlayerProfile};

#[test]
fn default_ui_config_enables_video_with_user_mpv_profile_and_larger_budget() {
    let config = UiConfig::default();

    assert!(config.video_preview.enabled);
    assert_eq!(config.video_preview.chunk_size_bytes, 4 * 1024 * 1024);
    assert_eq!(config.video_preview.max_chunks, 32);
    assert_eq!(
        config.video_preview.player_profile,
        VideoPlayerProfile::User
    );
}
