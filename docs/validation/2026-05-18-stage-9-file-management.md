# Stage 9 File Management Validation

## Metadata

- Date: 2026-05-18
- Stage: 9
- Status: complete
- Git commit: stage 9 commit (see `git log`)
- Host: local development workspace
- Command surface: cargo, tmux
- Sensitive output redacted: yes

## Objective

Add daily file-management operations to the Ratatui browser without turning destructive actions into accidental single-key operations: create directory, delete, rename, move, copy, multi-select, search, explicit confirmation, dry-run summaries, background execution, and cache refresh after mutation.

## Implemented Design

- PCS request layer now supports `mkdir`, `search`, batch `delete`, batch `copy`, and batch `move` request construction.
- Batch JSON matches the Go reference shapes: delete uses `{"list":[{"path":...}]}`, copy/move use `{"list":[{"from":...,"to":...}]}`.
- Remote operation plans live in core and provide dry-run lines, destructive classification, affected directory calculation, and item counts.
- `TaskManager` can track remote operation tasks in addition to download/upload tasks.
- TUI command mode supports `:mkdir`, `:rename`, `:copy`, `:move`, and `:rm`/`:delete`.
- Search mode supports `/keyword`; command mode supports `:search [-r] keyword`.
- Space toggles multi-select. If no explicit command paths are provided, copy/move/delete operate on the marked set, falling back to the current selected entry.
- Destructive delete never executes immediately. It enters Confirm mode and requires `y`; `n`/Esc cancel.
- Confirm mode prioritizes the dry-run summary and `y/n` prompt in the Tasks panel so the prompt remains visible in an 80x24-style layout.
- Remote operations and search run in background worker threads and report back through task events; navigation/rendering is not blocked by the network request.
- Operation completion refreshes every affected cached directory and refreshes the current directory when it is related to the mutation.

## TDD Evidence

RED examples:

```bash
cargo test -p baidupcsr-pcs --test stage9_file_ops
cargo test -p baidupcsr-core --test stage9_file_operations
cargo test -p baidupcsr-tui --test stage9_commands
cargo test -p baidupcsr-cli remote_command_
cargo test -p baidupcsr-tui --test stage5b_ratatui_render ratatui_render_prioritizes_confirm_prompt_in_tasks_panel
```

Observed failures covered:

- Missing PCS file-operation request builders and param JSON helpers.
- Missing operation response parser for `errno=0` and remote `error_code`.
- Missing core operation plan and remote operation task kind.
- Missing TUI command input, Enter/Backspace key events, multi-select, and confirm events.
- Missing CLI command parsing for selected-entry operations and explicit-path operations.
- Confirm mode summary existed but the visible Tasks panel hid the `y/n` prompt behind older task lines in a real 24-line smoke.

GREEN evidence:

```bash
cargo test -p baidupcsr-pcs --test stage9_file_ops
cargo test -p baidupcsr-core --test stage9_file_operations
cargo test -p baidupcsr-tui --test stage9_commands
cargo test -p baidupcsr-cli
cargo test -p baidupcsr-tui --test stage5b_ratatui_render
```

Observed result:

- PCS Stage 9 tests passed for request methods, URL query fields, reference-shaped JSON, and operation response parsing.
- Core Stage 9 tests passed for destructive delete planning, mkdir parent refresh, copy/move related directory calculation, empty target rejection, and remote operation task tracking.
- TUI Stage 9 tests passed for command submission, search submission, multi-select, and confirm acceptance/rejection.
- CLI tests passed for relative-path parsing, selected-entry fallback, explicit rename/copy/move/delete paths, recursive search parsing, and redacted task summaries.
- Render regression passed after making Confirm mode show the summary and `y/n` prompt before old task lines.

## Real tmux Smoke

Sessions:

- `baidupcsr-stage9-fileops`
- `baidupcsr-stage9-confirm-ui`

Redacted sequence:

1. Started `cargo run -p baidupcsr-cli -- tui` in tmux using the local BaiduPCS-Go active config.
2. In the active remote workdir, created a temporary smoke directory with `:mkdir`.
3. Renamed that temporary directory with explicit `:rename <from> <to>`.
4. Created a temporary destination directory and copied the renamed directory into it with explicit `:copy <src> <target-dir>`.
5. Created another temporary source directory and moved it into the destination directory with explicit `:move <src> <target-dir>`.
6. Ran a no-hit `/search` using a synthetic smoke keyword; search completed as a background remote task and showed zero entries without blocking the TUI.
7. Ran `:rm <temp-dir> <temp-dest-dir>`, verified the app entered `CONFIRM` mode rather than deleting immediately, then pressed `y`.
8. The delete task completed and the active directory refreshed back to its original item count, confirming the temporary smoke directories were cleaned.
9. A follow-up confirm-only tmux session opened `:rm <synthetic-nonexistent-smoke-path>`, verified the Tasks panel displayed both `delete 1 remote entries` and `press y to confirm, n to cancel`, then pressed `n` and exited.

Raw pane captures included private existing remote filenames from the active directory. They were inspected only for status fields and smoke-owned temporary names and are intentionally not stored here.

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
- `cargo clippy --workspace --all-targets -- -D warnings`: passed after removing `expect`/panic style from Stage 9 tests.
- `git diff --check`: passed.

## Redaction

No Cookie, BDUSS, STOKEN, access token, dlink, raw existing remote path, private filename, or file content is written here.

## Conclusion

Stage 9 passes the file-management gate. The implemented slice covers common TUI file management, confirmation for destructive delete, multi-select, search, background operation tasks, and cache refresh. Recycle/share/offline download are explicitly left for later migration as planned.
