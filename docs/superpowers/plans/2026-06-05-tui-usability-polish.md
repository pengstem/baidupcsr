# TUI Usability Polish Plan

**Date:** 2026-06-05

**Goal:** Turn the current Ratatui browser from a debug-state surface into a file-manager style UI: human-readable status and metadata, local list filtering on `/`, clear destructive confirmations, useful navigation for large directories, and visual states that match what users can act on.

**Scope guard:** Existing dirty PCS/preview range changes are not part of this pass. Stage commits should include only TUI usability code, tests, and move-log updates unless a dependency change is required for the stage.

## Stage 0: Plan Baseline

- Create this staged plan.
- Record the start in `docs/moves.md`.
- Commit the planning baseline before code changes.

## Stage 1: Human-Readable Status And Metadata

Requirements:

- Replace the bottom `Status mode=NORMAL path=... request=... cache_age=... tasks=...` debug string with a file-manager style line: colored mode, selected position such as `12/340`, selected count, current path, and short loading/error/task hints only when relevant.
- Move raw request/cache/task counters out of the status bar; keep Tasks for real user-facing task/notice lines.
- Format Unix `mtime` values as `YYYY-MM-DD HH:MM`.
- Format file sizes with decimal precision and `B/KiB/MiB/GiB/TiB`.
- Show Files empty/loading/error states inside the panel.
- Remove preview memory-accounting lines from normal metadata/text previews; keep video diagnostics visible.
- Record before/after tests and commit.

Validation:

- Focused TUI state/render tests for status, timestamp, size, empty/loading/error, Tasks, and Preview copy.
- `cargo test -p baidupcsr-tui`

## Stage 2: Local Filter On `/`

Requirements:

- Make `/` start an incremental local filter over the current directory entries.
- Filter as the user types without submitting a remote task.
- Preserve `:search <keyword>` as the remote recursive search command path.
- Render command/filter input in the bottom modeline with a visible cursor marker.
- Ensure selection maps to the filtered list while actions still operate on the real selected entry.
- Record before/after tests and commit.

Validation:

- State tests proving `/notes` filters current entries, `Enter` exits filter mode without `CommandSubmitted`, `Esc` clears the filter, and `:search notes` still submits the remote search command.
- Render tests proving the modeline input location and filtered count.
- `cargo test -p baidupcsr-tui --test stage9_commands`
- `cargo test -p baidupcsr-tui`

## Stage 3: Confirmation And Quit Safety

Requirements:

- Render destructive confirmations as centered modals with a red border, affected files, and explicit `[y] confirm / [n] cancel` actions.
- Stop using the Tasks box for confirmation UI.
- If `q` is pressed while background work is active, require an explicit quit confirmation instead of exiting immediately.
- Record before/after tests and commit.

Validation:

- Render tests for centered danger modal contents and absence from the Tasks panel.
- State tests for quit guard accept/reject behavior.
- `cargo test -p baidupcsr-tui`

## Stage 4: Large Directory Navigation And Selection Visibility

Requirements:

- Add `gg`, `G`, `Ctrl-d`, `Ctrl-u`, `PageUp`, `PageDown`, `Home`, and `End`.
- Add a scrollbar or equivalent position indicator for long file lists.
- Split cursor and mark indicators so the cursor does not hide multi-select state; marked rows need row-level visual styling.
- Make mouse wheel scrolling move the viewport/selection by page-sized deltas without pretending to be a single-key Up/Down action.
- Record before/after tests and commit.

Validation:

- State tests for each navigation key and marked selected row semantics.
- Render tests for scrollbar/position indicator and two-column marker visibility.
- `cargo test -p baidupcsr-tui`

## Stage 5: Focus, Theme, Help, Truncation, Final Gate

Requirements:

- Resolve the always-active border noise: either implement real Files/Preview focus with `Tab`, or remove the active/inactive border distinction. The planned direction is real `Tab` focus because Preview can receive scroll/navigation later.
- Make `THEME_PANEL` visually distinct from `THEME_BG` or delete the fake distinction; the planned direction is a slightly lighter panel.
- Update help text to include `?`, `Esc`, `Enter`, `y/n`, command/filter context, and paging keys.
- Add `+N more` indicators where task/preload/preview content is truncated.
- Re-run the focused TUI suite and workspace checks if unrelated dirty files do not block them.
- Record final validation and commit.

Validation:

- Render/state tests for focus border, theme color, help contents, and truncation indicators.
- `cargo test -p baidupcsr-tui`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --check`

## Completion Audit

Before marking the goal complete, audit each original numbered item:

- Items 1-6 must be directly implemented and verified.
- Items 7-17 must either be implemented and verified or explicitly remain in the active goal with evidence of the latest stage boundary.
- The final worktree must preserve pre-existing unrelated dirty PCS/preview changes.
