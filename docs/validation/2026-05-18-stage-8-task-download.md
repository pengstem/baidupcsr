# Stage 8 Task Download Validation

## Metadata

- Date: 2026-05-18
- Stage: 8
- Status: complete
- Git commit: stage 8 commit (see `git log`)
- Host: local development workspace
- Command surface: cargo, tmux
- Sensitive output redacted: yes

## Objective

Separate explicit transfer work from UI navigation: downloads must run as cancellable background tasks, show visible task state/progress/failure in the Ratatui UI, write only when the user explicitly requests a download, and leave Preview as a separate memory-only path.

## Implemented Design

- `TaskManager` tracks task id, kind, state, byte progress, speed, failure reason, cancellation request, retry parent, and active count.
- Download and upload share the same task model. Upload currently records the Go-compatible conflict policy surface (`skip`, `overwrite`, `rsync`) without starting network upload work in this stage.
- `d` starts an explicit download for the selected file; `x` requests cancellation of the latest queued/running transfer.
- Transfer work runs on a background worker and reports progress/completion/failure/cancellation back to the TUI through task events, so `j/k/h/l/r` navigation is not blocked by the download loop.
- The task panel shows recent transfer summaries, and the status line includes transfer count plus `d/x` key hints.
- Download output goes to `BAIDUPCSR_DOWNLOAD_DIR` when set, otherwise the BaiduPCS-Go `savedir`, otherwise the current directory.
- Download writes to `<target>.part`, resumes from existing partial length, fetches 1 MiB `Range` chunks through Stage 3 `locatedownload` dlinks, and renames to the final file only after all bytes are written.
- Download target names are sanitized before joining with the output directory, including the `.` and `..` edge cases.
- The dlink fetch path tries later locatedownload candidates after a network failure on an earlier candidate.
- Cancellation keeps the partial file for resume and records a visible reason. If cancellation races with an already completed worker, completion is allowed to land as a terminal state instead of leaving the task stuck in `cancelling`.
- Preview and download remain separate: Stage 6/7 preview code still uses bounded memory preview and loopback video preview paths, while explicit download is the only path that writes normal files.

## TDD Evidence

RED examples:

```bash
cargo test -p baidupcsr-core --test stage8_task_manager
cargo test -p baidupcsr-cli download_target_rejects_dot_path_components_as_filenames
cargo test -p baidupcsr-cli selected_path_for_refresh_uses_current_selection
```

Observed failures covered:

- Missing `TaskManager`, task ids, progress, cancellation, retry, and upload policy model.
- Missing `d`/`x` key actions and visible task lines in the TUI state.
- Missing explicit download target sanitization for `.` and `..`.
- A cancellation/completion race that could leave a task stuck in `cancelling`.
- Missing current-selection preservation when refreshing after a transfer event.
- A real dlink candidate failure during live smoke; the worker now tries later locatedownload candidates before failing the task.

GREEN evidence:

```bash
cargo test -p baidupcsr-cli
cargo test -p baidupcsr-core --test stage8_task_manager
cargo test -p baidupcsr-tui --test stage5_tui_state
cargo test -p baidupcsr-tui --test stage5b_ratatui_render
```

Observed result:

- CLI tests passed, including download target sanitization, redacted task summary, fallback dlink selection, and current-selection refresh helper.
- Core Stage 8 tests passed, including progress/speed, cancellation, late completion after cancellation request, retry, and upload policy recording.
- TUI state/render tests passed, including `d`/`x` keymap exposure and bottom task line state.

## Real tmux Smoke

Session:

- `baidupcsr-stage8-download`

Redacted sequence:

1. Started `cargo run -p baidupcsr-cli -- tui` in tmux with `BAIDUPCSR_DOWNLOAD_DIR=/tmp/baidupcsr-stage8-download-smoke`.
2. Loaded a real remote directory from the local BaiduPCS-Go active config.
3. Selected a real small file and pressed `d`, then moved selection with `j/k` while the task was active.
4. The TUI stayed responsive, the Tasks panel showed a running download, and the final task state became `download done` with bytes done equal to bytes total.
5. Verified the explicit output file existed under the smoke download directory and its local size matched the completed task byte count.
6. Selected a real large video file, pressed `d`, then pressed `x`.
7. The Tasks panel showed `download cancelled`, preserved a `.part` file for the cancelled task, and did not block directory navigation.
8. Cleaned the smoke output files and exited the tmux session.

An earlier smoke attempt failed with a redacted `network` task error because the first locatedownload dlink candidate was not usable; after adding fallback across candidates, the same real-config path completed the small download. Raw pane captures included private remote names and are intentionally not stored here.

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
- `cargo clippy --workspace --all-targets -- -D warnings`: passed after replacing a manual zero-division guard with `checked_div`.
- `git diff --check`: passed.

## Redaction

No Cookie, BDUSS, STOKEN, access token, dlink, loopback token, raw remote path, raw filename, or downloaded file content is written here.

## Conclusion

Stage 8 passes the explicit transfer gate: the implemented slice covers the background task model, explicit real download path, visible task UI, cancellation, resume partials, and the upload conflict-policy surface. Real network upload execution remains outside this stage.
