# MPV Profile And Smoothness Fix Validation

## Metadata

- Date: 2026-05-18
- Scope: mpv launch profile and bounded video cache defaults
- Sensitive output redacted: yes

## Problem

Two user-visible issues were reported after Stage 10:

- mpv did not use the user's normal style or plugins.
- playback stalled after only a few frames.

## Root Cause

- The mpv launch profile always passed `--no-config`, `--load-scripts=no`, and
  `--osc=no`. This intentionally disabled user config, scripts, and OSD styling
  for the earlier strict smoke, but it made the player feel unlike the user's
  normal mpv.
- The default video cache budget was `256 KiB * 4 = 1 MiB`. The loopback proxy
  clamps each mpv `Range` request to one chunk, so this caused many small remote
  Range requests against Baidu's dlink and made playback sensitive to network
  round-trip latency.

## Fix

- Default mpv profile is now `user`: mpv can load normal user config, scripts,
  and OSD style.
- The older no-config/no-scripts launch is still available through
  `video_player_profile = "isolated"` in the Rust sidecar config.
- Default video cache is now `4 MiB * 16 = 64 MiB`.
- mpv launch now keeps `--cache-on-disk=no`, uses the app loopback URL, and adds
  `--demuxer-readahead-secs=20` plus `--cache-pause=no`.
- `doctor --redact` reports the active profile and video cache budget.

## RED Evidence

Tests were added first and observed failing because the profile and config field
did not exist yet:

```text
cargo test -p baidupcsr-config --test stage2_config
cargo test -p baidupcsr-config --test stage9_5_video_defaults
cargo test -p baidupcsr-preview --test stage7_video_preview
cargo test -p baidupcsr-cli --test stage10_doctor_release
```

Observed failures included missing `VideoPlayerProfile`, missing
`player_profile`, missing `MpvLaunchProfile`, missing
`player_launch_with_profile`, and `doctor` missing `profile=user`.

## Verification

Commands run before commit:

```text
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p baidupcsr-cli -- doctor --redact
cargo test -p baidupcsr-cli live_video_dlink_range_smoke_is_redacted -- --ignored --nocapture
git diff --check
```

Results:

- Formatting check passed.
- Workspace tests passed, including profile-aware mpv launch tests.
- Clippy passed with `-D warnings`.
- `doctor --redact` reported `video_preview: enabled profile=user
  chunk=4194304 chunks=16 budget=67108864`.
- The ignored live dlink Range smoke passed and printed only URL count and byte
  count.
- Whitespace check passed.

## Notes

The player still receives only `http://127.0.0.1:<port>/preview/video/<token>`;
mpv never receives the Baidu dlink. In `profile=user`, local mpv scripts are
trusted user code and can have their own side effects. Use `profile=isolated`
when that is not acceptable.
