# Video Playback Gateway Task 1 Validation

## Metadata

- Date: 2026-05-18
- Stage: Video playback gateway Task 1
- Git commit: based on `66d0d03`, commit pending
- Host: nastem-arch
- Command surface: cargo tests, clippy, tmux TUI smoke
- Sensitive output redacted: yes

## Objective

Prove the first playback-gateway slice adds a diagnosable video playback contract without exposing dlinks, tokens, cookies, or local private remote names.

Task 1 is intentionally limited to playback mode and diagnostics:

- TUI video lines include explicit `playback=...`, `mpv_profile=...`, and `player_log=...`.
- mpv stderr is captured into a bounded in-memory ring instead of being discarded.
- player logs redact URLs and token-like fields before they can reach the TUI.
- buffering semantics are not changed yet; interval buffering and prefetch are later tasks.

## Commands

```bash
cargo test -p baidupcsr-preview --test stage7_video_preview player_log_ring_keeps_recent_sanitized_lines
cargo test -p baidupcsr-preview --test stage7_video_preview playback_status_marks_stalled_after_no_progress_timeout
cargo test -p baidupcsr-preview --test stage7_video_preview player_log_ring_redacts_token_like_fields
cargo test -p baidupcsr-preview --test stage7_video_preview
cargo test -p baidupcsr-cli active_player_log_line_uses_last_sanitized_player_line
cargo test -p baidupcsr-tui --test stage5b_ratatui_render ratatui_render_keeps_video_playback_diagnostics_visible
cargo test -p baidupcsr-preview --test stage7_video_preview interval_buffer
cargo test -p baidupcsr-preview --test stage7_video_preview range_parser_accepts_open_ended_and_suffix_ranges
cargo test -p baidupcsr-preview --test stage7_video_preview suffix_proxy_range_resolves_against_file_tail
```

Full gate commands to run before commit:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Live smoke to run before commit:

```bash
tmux new-session -d -s baidupcsr-video-gateway 'cargo run -p baidupcsr-cli -- tui'
tmux send-keys -t baidupcsr-video-gateway '<navigation to a real video>' Enter
tmux send-keys -t baidupcsr-video-gateway p
tmux capture-pane -pt baidupcsr-video-gateway
pgrep -af 'mpv|baidupcsr-cli tui|target/debug/baidupcsr-cli'
tmux send-keys -t baidupcsr-video-gateway q
tmux kill-session -t baidupcsr-video-gateway
```

## Evidence Summary

- RED: CLI test failed because `active_player_log_line` did not exist.
- RED: token-like player-log test failed because `BDUSS=...`, `token=...`, and `cookie=...` were not redacted.
- GREEN: `cargo test -p baidupcsr-preview --test stage7_video_preview` passed with 13 tests.
- GREEN: `cargo test -p baidupcsr-cli active_player_log_line_uses_last_sanitized_player_line` passed.
- GREEN: `cargo test -p baidupcsr-tui --test stage5b_ratatui_render ratatui_render_keeps_video_playback_diagnostics_visible` passed.
- RED: interval-buffer test failed because `VideoIntervalBuffer` did not exist.
- GREEN: `cargo test -p baidupcsr-preview --test stage7_video_preview interval_buffer` passed.
- GREEN: `cargo test -p baidupcsr-preview --test stage7_video_preview` passed with 15 tests after Task 2.
- RED: Range-parser test failed because `parse_video_range_header` and `RequestedVideoRange` did not exist.
- GREEN: open-ended and suffix Range tests passed; stage7 preview tests passed with 17 tests after Task 3.
- Full gate: `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed before the live smoke.
- Live tmux smoke: started the TUI with the local BaiduPCS-Go config, navigated to a real video, pressed `p`, and verified the visible 80x24 panel showed `video=loopback seek=true`, `playback=playing`, `mpv_profile=user`, and `player_log=none`.
- Live process check: mpv was launched with only a `http://127.0.0.1:<port>/preview/video/<token>` media URL; no dlink was present in process args.
- Live listener check: the loopback server listened on `127.0.0.1:<port>` under `baidupcsr-cli`.
- Cleanup: quitting the tmux TUI removed the mpv process and the CLI loopback listener.
- Important output shape: TUI video diagnostics now have `video=loopback seek=true`, `playback=...`, `mpv_profile=...`, `player_log=...`, `listen=...`, `player=...`, `range=...`, `cache=...`, and `disk=false error=...`.
- Files inspected: `crates/baidupcsr-preview/src/lib.rs`, `crates/baidupcsr-preview/tests/stage7_video_preview.rs`, `crates/baidupcsr-cli/src/main.rs`.

## Redaction

This record does not include raw Cookie, BDUSS, STOKEN, SBOXTKN, access token, dlink, mpv private URL, local username-derived path, or remote filename values.

## Conclusion

Task 1 passes unit, render, full workspace, clippy, diff, and live tmux validation. Task 2 adds the interval-buffer contract. Task 3 adds open-ended and suffix Range request semantics. The gateway is not fully smooth yet: active prefetch and live retesting remain.
