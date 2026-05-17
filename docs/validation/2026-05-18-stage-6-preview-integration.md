# Stage 6 Preview Integration Validation

## Metadata

- Date: 2026-05-18
- Stage: 6
- Status: complete
- Git commit: stage 6 integration commit (see `git log`)
- Host: local development workspace
- Command surface: cargo, tmux, baidupcs search
- Sensitive output redacted: yes

## Objective

Complete Stage 6 beyond the earlier memory-manager foundation: real remote text bytes must be fetched with a bounded Range request, rendered in the Ratatui Preview pane, cached in memory, and kept separate from ordinary download/open actions.

## Implemented Design

- `PreviewPlan` classifies selected entries before any network fetch: directories and unsupported/binary files use metadata-only preview, small text files use bounded text preview, and files over the 16 MiB preview budget never fetch content.
- `ByteRange` and `BlockingPcsClient::download_range` issue read-only PCS download requests with a `Range` header and a bounded local reader, so an ignored remote Range response still cannot exceed the requested preview cap.
- `BrowserRuntime` binds preview requests to the currently selected entry with account + `fs_id` + `mtime` + size keys, then rejects stale results before updating the UI.
- Preview fetches use a cooperative cancellation epoch: a selection change marks older preview workers stale before network work and between bounded body reads.
- Text preview results go through `PreviewManager` and its in-memory LRU; returning to an already previewed file restores content without starting a visible background task.
- The TUI Preview pane now displays budget status before content, so `preview`, `fetched`, `limit`, `truncated`, and `disk` remain visible in an 80x24 terminal.
- Preview/Open/Download remain separate at this stage: preview uses `download_range` directly for bounded bytes, `l` on a file does not enter/download/open, and no normal transfer task is invoked.

## TDD Evidence

RED examples:

```bash
cargo test -p baidupcsr-preview --test stage6_preview_manager
cargo test -p baidupcsr-pcs --test stage3_api
cargo test -p baidupcsr-tui --test stage5_tui_state
cargo test -p baidupcsr-tui --test stage5b_ratatui_render
cargo test -p baidupcsr-tui --test stage5b_ratatui_render ratatui_render_keeps_preview_budget_visible_with_text_content
cargo test -p baidupcsr-pcs --test stage3_api download_range_can_be_cancelled_before_network_work
```

Observed failures covered missing preview planning, missing bounded Range helper, missing cooperative cancellation, missing TUI preview content state, missing rendered text preview, and a real 80x24 UI regression where preview content pushed `disk=false` below the visible pane.

GREEN:

```bash
cargo test -p baidupcsr-tui --test stage5b_ratatui_render ratatui_render_keeps_preview_budget_visible_with_text_content
```

Observed result:

- The focused regression test passed after moving budget status above preview content and truncating long preview labels/paths.

## Real tmux Smoke

The smoke used the local BaiduPCS-Go active config and a real read-only remote tree. A prior `baidupcs search -r` helped locate a small text file, but raw search output contained private remote filenames and is not stored here.

Session:

- `baidupcsr-stage6-preview-20260518`

Redacted sequence:

1. Started `cargo run -p baidupcsr-cli -- tui` in tmux.
2. Entered a real nested remote directory using `j/l/l/l`.
3. Selected a real small `.txt` file; Preview showed `preview=text`, `fetched=664`, `limit=16777216`, `truncated=false`, and `disk=false`.
4. The first text lines rendered below the budget status in the Preview pane.
5. Quickly moved from the text file to a neighboring non-text file with `j/j/k`; Preview changed to metadata for the current selected file and did not keep showing the old text content.
6. Moved back to the text file with `k`; Preview restored the same text with `background=0`, confirming the visible path used memory cache rather than a new blocking fetch.
7. Checked `/tmp` top-level for recent `baidupcsr*` and `preview*` files; no preview temp files were created.
8. Pressed `q`; `tmux has-session` reported the session was gone.

The same session was rerun after adding cooperative preview cancellation; the final code again showed real text preview and selection-bound metadata after fast cursor movement.

Raw pane captures included private remote paths and user file content. They were inspected only for status fields and are intentionally not stored.

## Verification Commands

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Observed result:

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.

## Redaction

No Cookie, BDUSS, STOKEN, access token, dlink, UID, raw remote path, or raw preview file content is written here.

## Conclusion

Stage 6 passes the tightened gate: real config-backed text preview works in the Ratatui UI, preview reads are bounded and memory-only, large/binary entries stay metadata-only, quick selection changes do not show stale content, and tmux smoke confirmed no preview files were created.
