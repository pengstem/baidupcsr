# Stage 5b Real Browser Validation

## Metadata

- Date: 2026-05-18
- Stage: 5b
- Status: complete
- Git commit: stage 5b commit (see `git log`)
- Host: local development workspace
- Command surface: cargo, tmux
- Sensitive output redacted: yes

## Yazi Calibration

Checked yazi implementation before changing the TUI path:

- `yazi-core/src/tab/folder.rs`: folder owns cursor, offset, stage, and keeps the selected row visible through scroll logic.
- `yazi-fs/src/files.rs`: file lists use ticket/revision style guards so stale partial updates do not overwrite newer state.
- `yazi-widgets/src/scrollable.rs`: scrolling is a first-class model concern, not a side effect of drawing all rows.
- `yazi-fm/src/dispatcher.rs` and `executor.rs`: key input dispatches actions; async results return through an event path instead of blocking render.

Sources:

- https://github.com/sxyazi/yazi/blob/main/yazi-core/src/tab/folder.rs
- https://github.com/sxyazi/yazi/blob/main/yazi-fs/src/files.rs
- https://github.com/sxyazi/yazi/blob/main/yazi-widgets/src/scrollable.rs
- https://github.com/sxyazi/yazi/blob/main/yazi-fm/src/dispatcher.rs
- https://github.com/sxyazi/yazi/blob/main/yazi-fm/src/executor.rs

## Implemented Design

- Runtime still uses the local BaiduPCS-Go active config; no demo/mock data exists on the `tui` runtime path.
- Directory list requests now run in background workers and return through a channel into the TUI loop.
- `DirectoryCache` is used on the product path: entering or refreshing a path immediately shows cached entries when present, then refreshes in the background.
- Refresh generations reject stale results.
- Moving with `j/k` prefetches the selected directory without changing the current folder.
- `l` enters only directories; pressing `l` on a file leaves the path unchanged and shows a visible Tasks notice.
- `h` returns to the parent and restores the child directory selection when possible.
- Files render directory-first, with strong selected-row background, truncation, scrolling, visible cache age, request state, and Preview metadata.

## TDD Evidence

RED examples:

```bash
cargo test -p baidupcsr-core --test stage4_directory_cache
cargo test -p baidupcsr-tui --test stage5_tui_state
cargo test -p baidupcsr-tui --test stage5b_ratatui_render
```

Observed failures covered missing cache error preservation, missing request-state APIs, missing cached-refresh UI, and missing scrolling/truncation/highlight behavior.

GREEN:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Observed result:

- `cargo test --workspace`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.

## Real tmux Smoke

First sandboxed run correctly showed an in-TUI network error rather than fake data. The real network smoke was rerun with an escalated tmux session because the workspace sandbox blocks ordinary network access.

Sessions:

- `baidupcsr-stage5b-20260518-real2`
- `baidupcsr-stage5b-20260518-keys`

Redacted sequence:

1. Started `cargo run -p baidupcsr-cli -- tui`.
2. TUI loaded the active BaiduPCS-Go workdir and displayed a real remote list.
3. `l` on a real directory entered that directory and displayed its real contents.
4. `l` on a regular file did not change directories and showed a visible `preview_pending` notice in Tasks.
5. `h` returned to the parent directory and restored the child directory selection.
6. `r` refreshed the current directory from the real API; cached entries remained visible.
7. `j` moved selection down and Preview followed the selected entry.
8. `k` moved selection back up and Preview followed again.
9. `q` exited; `tmux has-session` reported the session was gone, proving terminal exit completed.

Raw pane captures included private remote filenames and are intentionally not stored.

## Redaction

No Cookie, BDUSS, STOKEN, access token, dlink, UID, or raw remote filename list is written here.

## Conclusion

Stage 5b passes the updated gate: real config-backed runtime, real read-only remote list, `j/k/l/h/r/q`, async channel-based directory loads, cache-backed refresh, visible failure/loading states, and a usable Ratatui layout.
