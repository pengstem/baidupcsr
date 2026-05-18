# TUI UI Quality Validation

## Metadata

- Date: 2026-05-18
- Stage: TUI UI quality pass
- Git commit: commit pending
- Host: nastem-arch
- Command surface: cargo tests, clippy, tmux TUI smoke
- Sensitive output redacted: yes

## Objective

Validate the Focused Browser UI pass:

- `?` opens help, but help is not visibly promoted on the normal screen.
- Arrow keys mirror Vim navigation.
- Mouse click/wheel events select and scroll through file rows.
- Wide layout uses two main panes instead of the previous context rail.
- Preview labels are human-readable and long names keep useful suffixes.
- Preview planning covers audio, Office-like documents, e-books, fonts, checksums, torrents, and playlists.

## Focused Commands

```bash
cargo test -p baidupcsr-tui --test stage5_tui_state question_mark_toggles_hidden_help_overlay
cargo test -p baidupcsr-tui --test stage5_tui_state arrow_keys_mirror_vim_navigation_actions
cargo test -p baidupcsr-tui --test stage5_tui_state mouse_click_selects_visible_file_row
cargo test -p baidupcsr-tui --test stage5_tui_state mouse_wheel_moves_selection_like_keyboard_navigation
cargo test -p baidupcsr-tui --test stage5b_ratatui_render help_overlay_is_hidden_until_question_mark
cargo test -p baidupcsr-tui --test stage5b_ratatui_render
cargo test -p baidupcsr-tui --test stage9_5_tui_quality
cargo test -p baidupcsr-preview --test stage9_5_preview_coverage
cargo test -p baidupcsr-cli preview_kind_mapping_preserves_all_preview_boundaries
```

## RED/GREEN Evidence

- RED: `question_mark_toggles_hidden_help_overlay` failed because `TuiApp::help_visible` and help state did not exist.
- RED: `arrow_keys_mirror_vim_navigation_actions` failed because `KeyEvent::{Up,Down,Left,Right}` did not exist.
- RED: `help_overlay_is_hidden_until_question_mark` failed because pressing `?` did not render any `Shortcuts` overlay.
- GREEN: hidden help and arrow-key parity tests passed after adding help state, arrow events, and a compact overlay.
- RED: mouse tests failed because `MouseInput`, `MouseInputKind`, `handle_mouse_event`, and `KeyAction::Select` did not exist.
- GREEN: mouse click and wheel tests passed after adding file-row hit testing and mapping mouse selection into the same runtime refresh path.
- RED: focused layout tests failed because the wide layout still rendered `Context`, and long preview names lost the `.mkv` suffix.
- GREEN: focused layout and render suites passed after switching wide layout to Files/Preview panes, shortening the status line, and using middle truncation in Preview fields.
- RED: preview coverage tests failed because `PreviewKind::{Audio,Document,Ebook,Font}` did not exist.
- GREEN: preview coverage passed after adding those kinds and classifying audio, Office-like documents, e-books, fonts, checksums, torrents, and playlists.

## Live Tmux Smoke

Commands:

```bash
tmux new-session -d -s baidupcsr-ui-quality 'cargo run -p baidupcsr-cli -- tui'
tmux resize-window -t baidupcsr-ui-quality -x 120 -y 32
tmux capture-pane -pt baidupcsr-ui-quality
tmux send-keys -t baidupcsr-ui-quality '?'
tmux capture-pane -pt baidupcsr-ui-quality
tmux send-keys -t baidupcsr-ui-quality Escape Down Up Right Left
tmux send-keys -t baidupcsr-ui-quality Down
tmux capture-pane -pt baidupcsr-ui-quality
tmux send-keys -t baidupcsr-ui-quality q
```

Evidence summary:

- The real TUI loaded the local BaiduPCS-Go config and showed a real remote directory.
- Wide layout rendered only `Files` and `Preview` as the main panes; no `Context` rail was visible.
- The normal status bar was short and did not show a prominent shortcut list.
- Pressing `?` opened a `Shortcuts` overlay containing navigation, actions, and mouse entries.
- Esc closed the overlay.
- Down moved the selected row and refreshed Preview to the new selected directory.
- Mouse behavior was not injected through tmux because terminal mouse-event forwarding is environment-specific; automated tests cover the coordinate-to-selection logic and terminal startup enables Crossterm mouse capture.
- The tmux session exited cleanly after `q`.

## Full Gate

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Result: all passed.

## Redaction

This record does not include raw Cookie, BDUSS, STOKEN, SBOXTKN, access token, dlink, loopback token, or full private remote filenames beyond generic UI shape examples.

## Conclusion

The Focused Browser UI pass has automated coverage for the new input, layout, and preview-classification behavior, plus a real tmux smoke for the visible TUI flow. The full workspace gate passed before commit.
