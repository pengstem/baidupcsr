# Codex Session Retrospective

Date: 2026-06-13

Scope: historical Codex session logs for `/home/nastem/Project/baidupcsr`, plus the baidupcsr rollout summaries and current git history.

## Executive Summary

The project history is not mainly "random debugging." The dominant effort was building and productizing a Rust/Ratatui file-manager-like TUI, then paying down the complexity created by that rapid buildout.

The practical story is:

- Early work was a stage-gated rewrite and prototype-to-product push: config compatibility, PCS API parsing, directory cache, TUI state/rendering, preview, video, download tasks, file operations, release docs, and real tmux validation.
- Later work shifted to stabilization and ergonomics: video seek correctness, workdir persistence, local filtering, confirmations, focus, breadcrumbs, mouse/scrollbar behavior, Ratatui widgets, transfer panels, and popup input.
- Maintenance became substantial by June 7-8: `cargo shear`, MSRV fixes, stale dependency cleanup, dead-helper audits, DTO narrowing, and strict validation.
- A hidden operational stream appeared on June 8-9: live Baidu netdisk organization using `baidupcs`, including `/电影`, `/电视剧`, `/学习资料`, `/待删除`, and provider-limit workarounds.

Key productivity finding: session count alone is misleading. A naive exact-cwd classifier labeled almost everything as operational because this repo is a netdisk client. Reclustered by actual user intent and commands, feature work consumed about half of command-weighted effort, refactoring about 29%, operations about 13%, and direct debugging about 7%.

## Productivity Metrics

### Data Shape

| Metric | Count | Notes |
|---|---:|---|
| Exact-cwd raw sessions | 18 | Sessions whose metadata cwd was `/home/nastem/Project/baidupcsr`. |
| Active work sessions | 13 | Excludes approval/governance/meta rows and this retrospective audit. |
| Command proxy in active sessions | 6,254 | Used as effort proxy, not wall-clock truth. |
| Error/retry signal in active sessions | 2,336 | Includes failed tests, compiler errors, command failures, provider errors, and validation churn. |
| Git commits in repo history | 46 | Commit counts overrepresent staged feature slices by design. |

### Command-Weighted Effort

| Category | Active Sessions | Commands | Effort Share | Error Signals | Commit Count |
|---|---:|---:|---:|---:|---:|
| Feature Enhancement / UX | 4 | 3,154 | 50.4% | 1,104 | 29 |
| Refactoring / Technical Debt | 3 | 1,830 | 29.3% | 963 | 5 |
| Operational / Netdisk + API Research | 3 | 818 | 13.1% | 90 | 0 |
| Debugging / Stabilization | 3 | 452 | 7.2% | 179 | 3 |
| Total | 13 | 6,254 | 100% | 2,336 | 37 |

Effort chart:

```text
Feature Enhancement / UX          50.4%  #########################
Refactoring / Technical Debt      29.3%  ###############
Operational / Netdisk + Research  13.1%  #######
Debugging / Stabilization          7.2%  ####
```

### Commit Mix

Git history has 46 commits:

| Commit Class | Commits | Interpretation |
|---|---:|---|
| Feature / UX | 33 | Stage implementation, preview/download/file ops, TUI polish, search, input, panels. |
| Debugging / Stabilization | 6 | mpv/video, video load, mouse, dependency fix. |
| Docs / Planning | 5 | Stage plans, validation, architecture, UI quality plan. |
| Refactoring / Technical Debt | 2 | Dead code/helper cleanup commits. |

The commit mix confirms the project optimized for small staged feature slices. Command-weighted effort shows the cleanup work was much larger than its commit count suggests.

## Timeline

| Date | Focus | Milestone |
|---|---|---|
| 2026-05-17 to 2026-05-18 | Rust/Ratatui rewrite and staged prototype build | Stage 0-10: workspace, config compatibility, PCS API, directory cache, TUI state/render, preview, video core, download tasks, file operations, quality cache preview, release readiness, docs, and real tmux validation. |
| 2026-05-28 to 2026-05-29 | Video runtime stabilization | Fixed large-video far seeks by handling `Range: bytes=N-` as open-ended playback windows, added dlink candidate fallback, and persisted current TUI workdir to config. |
| 2026-05-30 | Broken preview investigation marker | Captured a user-reported preview outage; little tool activity survived in the log. |
| 2026-06-05 | Staged TUI usability polish | Plan + move log + commits for human-readable status, timestamps, sizes, local filtering, confirmation modal, guarded quit, large-directory navigation, focus, help, and truncation markers. |
| 2026-06-07 | Consolidation and interaction correctness | `cargo shear` stale dependency fix, preview/open/download boundary work, Ratatui Table/Scrollbar/Gauge/Sparkline migration, mouse scrollbar dragging, transfer panel split, and first dead-helper cleanup. |
| 2026-06-08 to 2026-06-09 | Peak mixed work | Large dead-function audit, directory-size experiments, recursive search/keymap/popup input polish, movie/TV netdisk reorg, learning-material deep sort, and `/待删除` centralization. |
| 2026-06-13 | Retrospective | Historical audit and guideline extraction. |

### Macro Shifts

1. Prototype/build -> runtime stabilization.
   The early stage plan quickly became a working app. The next failures came from real mpv, real PCS responses, real config, and real terminal behavior.

2. Feature polish -> interaction correctness.
   Once the TUI existed, work moved toward file-manager expectations: local filtering, clickable breadcrumbs, stable scrollbars, focus, popup inputs, and transfer panels.

3. Implementation -> maintenance discipline.
   `cargo shear`, strict clippy, MSRV fixes, dead-helper sweeps, DTO narrowing, and split commits became necessary because the first buildout produced many temporary seams.

4. App development -> operator workflow.
   The repo became the execution base for live `baidupcs` operations. This required a different validation style: `ls`, `tree`, `meta`, narrow `match`, source/destination checks, and no hard deletes.

## Hall Of Shame

### 1. Runtime Bugs Hidden Behind Passing Unit Tests

Symptoms:

- Large video seek froze near the end of a file.
- Tests covered range parsing, but the real mpv loopback path still mishandled `Range: bytes=N-`.
- One failing locatedownload dlink candidate could fail playback even when later candidates worked.

Root cause:

The unit model and the real player/proxy/PCS path drifted. Open-ended playback requests were treated like small preview chunks, and candidate fallback was not exercised by the primary tests.

Prevention:

- Model requested ranges as explicit protocol variants: explicit, open-ended, suffix.
- Add tests for far seeks, invalid ranges, cache-window resolution, and candidate fallback.
- Any video-path change must include a live smoke through `cargo run -p baidupcsr-cli -- tui`, mpv loopback URL inspection, and `206 Partial Content` checks where possible.

### 2. TUI State Changed, But The Screen Did Not

Symptoms:

- Mouse wheel mutated internal offsets without visible follow-through.
- Scrollbar thumb position used raw viewport offset instead of track-proportional position.
- Mouse drag/release was missing from the runtime mapping.
- Transfer metrics were squeezed into the wrong panel.
- Text input leaked into the bottom status line.

Root cause:

TUI tests often validated state transitions but not the final rendered buffer. Layout math was duplicated or inferred instead of shared between rendering and hit-testing.

Prevention:

- Every TUI interaction change needs both event/state tests and Ratatui render-buffer assertions.
- Hit-testing must use the same layout helpers as rendering.
- Mouse features need click, drag, release, wheel, and short-terminal cases.
- Status is status only. Text input should render as an overlay/popup with `Clear`, after the main panes.

### 3. PCS Provider Limits Looked Like App Bugs

Symptoms:

- Broad recursive `baidupcs search` returned `31034 pcs refuse service`.
- Some `mv` operations looked incomplete because old listings lagged behind.
- Sandboxed `baidupcs meta` failed because config/state access and DNS were restricted.
- Directory recursive size was expected but not exposed as a single `meta` field.

Root cause:

The Baidu PCS surface is rate-limited, eventually consistent, and not equivalent to a local filesystem API. Broad queries and immediate post-move listings are unreliable evidence.

Prevention:

- Prefer shallow `ls`/`tree` and narrow `match`/`search -path=...` over root-wide recursive search.
- After a move, verify direct source and destination paths before retrying.
- Treat recursive directory total size as computed data requiring traversal, cancellation, caching, and progress.
- Move delete candidates to `/待删除`; never hard-delete during organization work unless explicitly confirmed.

### 4. Tooling And MSRV Drift

Symptoms:

- `Option::is_none_or` passed local intuition but violated Rust 1.75 MSRV.
- `cargo shear` caught a stale `baidupcsr-core` dependency in the TUI crate.
- Release build hit a transient ThinLTO / `aws_lc_rs` / `Broken module found` failure.
- A combined cargo test filter failed because Cargo accepts one test filter expression.

Root cause:

Fast feature work outran the repository's pinned compatibility constraints and tool-specific behavior.

Prevention:

- Treat `rust-version = "1.75"` as a hard design constraint.
- Use explicit `match`/stable APIs instead of newer convenience methods unless MSRV is verified.
- Run `cargo shear` for dependency hygiene after crate-boundary churn.
- Retry a single unchanged release build once for known ThinLTO-like flake before debugging the patch.

### 5. Dead-Code Cleanup False Positives

Symptoms:

- Raw call counting flagged iterator/function-pointer uses and serde hooks as dead.
- Large deletion patches broke impl braces and dropped adjacent `#[test]` attributes.
- Removing fields failed until every constructor/test fixture was updated.

Root cause:

Syntactic signals were used before semantic verification. In Rust, important uses may appear in attributes, function pointers, iterator adapters, public DTO boundaries, or tests.

Prevention:

- Treat call counts as candidate generation only.
- Check exact call sites, serde attributes, iterator adapters, public DTO consumers, and tests before deleting.
- Run `cargo fmt --all` immediately after large deletions to catch structural damage early.
- Use smaller file-specific patches and rerun focused tests after each cleanup cluster.

## Lessons Learned

- Real runtime proof matters most where this project crosses process or provider boundaries: mpv, loopback HTTP, Baidu PCS, terminal mouse input, and config persistence.
- State and render are separate failure surfaces in Ratatui. A test that proves state changed is not enough.
- The best abstractions in this repo are boundary adapters, not thin wrappers. Preserve helpers that encode config discovery, platform behavior, protocol semantics, or render layout.
- Cleanup is valuable, but the safe workflow is discovery -> classify -> delete only proven-safe items -> validate broadly.
- The user expects live operational work to be evidence-backed and reversible: readable names, direct checks, `/待删除` for deletion candidates, and no destructive shortcuts.

## Knowledge Base Update: Developer Docs

Ready to paste into team developer documentation.

````markdown
## baidupcsr Engineering Guidelines

### Validation Tiers

- Pure parser/model changes: run focused crate tests, then `cargo test --workspace`.
- TUI state changes: add or update state tests in `crates/baidupcsr-tui/tests/stage5_tui_state.rs`.
- TUI render/layout changes: add Ratatui buffer assertions in `stage5b_ratatui_render.rs`; include short-terminal cases for fixed-height panels.
- Runtime boundary changes: validate the actual path, not only the unit model. For video, use the TUI/mpv/loopback flow. For PCS, use narrow `baidupcs ls/tree/meta/match` checks.
- Cleanup changes: run `cargo fmt --all` early, then strict workspace validation before committing.

Standard full gate:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
git diff --check
cargo run -p baidupcsr-cli -- doctor --redact
```

### TUI Rules

- Keep TUI state, runtime events, and rendering boundaries explicit.
- Mouse hit-testing must use the same layout helpers as rendering.
- Every mouse feature must cover click, drag, release, wheel, and render-position behavior.
- Update viewport, selection, preview refresh, and preload refresh together when interaction changes what the user sees.
- Do not overload the bottom status line with text input. Render command/filter/search/rename input as popup overlays with `Clear`.
- When adding fixed-height panels, test short terminal layouts so new panels are not compressed away.

### Preview, Open, Download, And Video

- Keep `Preview`, `Open`, and `Download` separate.
- Preview is read-only and bounded. Open may use `/tmp` for local programs that require a path. Download writes to the configured download location.
- Video is a strict loopback/mpv path by default; mpv must receive only a local loopback URL, not a raw Baidu dlink.
- Model HTTP range shapes explicitly: explicit, open-ended, and suffix.
- For large video changes, smoke-test real mpv seek behavior and verify `206 Partial Content` responses.

### Baidu PCS Operations

- Prefer narrow `ls`, `tree`, `meta`, and `match` checks. Avoid broad recursive root searches unless there is no alternative.
- Treat `31034 pcs refuse service` as a provider-limit signal. Narrow the query instead of retrying the same broad search.
- After `mv`, verify both source and destination directly before retrying; listings can lag.
- Directory recursive totals are computed data. Current metadata/list paths do not expose a single recursive size field.
- Move delete candidates into `/待删除`; keep uncertain content in `/待处理` or an archive bucket.

### Cleanup And Refactoring

- `cargo clippy -W dead_code` is not enough for cleanup discovery in this repo.
- Use raw symbol counts only to generate candidates.
- Before deleting, check function-pointer uses, iterator adapters, serde attributes, tests, public DTO consumers, and boundary semantics.
- Preserve one-call helpers when they isolate config discovery, platform behavior, protocol decisions, runtime events, or render layout.
- Split feature commits from lint/dependency cleanup when both are present.
````

## Knowledge Base Update: agent.md

No `agent.md` / `AGENTS.md` file exists in the repo today. This block is ready to paste if one is added later.

```markdown
# Agent Guidelines For baidupcsr

- Always inspect `git status --short` first. Preserve unrelated dirty work.
- For large TUI/UI tasks, write or update a dated plan under `docs/superpowers/plans/`, record progress in `docs/moves.md`, and keep one coherent commit per stage when the user asks for staged execution.
- Do not stop at static reasoning for runtime bugs. Reproduce through the real command path when feasible, especially `cargo run -p baidupcsr-cli -- tui`, mpv loopback playback, `doctor --redact`, or live `baidupcs` probes.
- Treat TUI state and render output as separate contracts. Add focused state tests plus Ratatui buffer/render tests for layout, scrolling, mouse, focus, overlays, and short-terminal behavior.
- Keep hit-testing and rendering on shared layout helpers. Never duplicate rectangle math for clickable breadcrumbs, files rows, preview scrollbars, or transfer panels.
- For video changes, keep Preview/Play/Open/Download semantics separate. mpv should receive a loopback URL. Test explicit, open-ended, and suffix ranges.
- For PCS operations, avoid broad recursive searches. Use narrow `ls`, `tree`, `meta`, `match`, and path-scoped search. Verify source and destination after moves because listings can lag.
- Never hard-delete netdisk cleanup candidates unless the user gives explicit final confirmation. Move clear trash into `/待删除`; keep ambiguous items out of deletion.
- For dead-code cleanup, start with a read-only candidate sweep. Separate dead code, thin wrappers, constant returns, one-call helpers, and boundary adapters. Delete only after exact consumer checks.
- After large deletions, run `cargo fmt --all` immediately to catch broken braces or adjacent test attributes, then run focused tests and the full gate.
- Respect Rust MSRV `1.75`; avoid newer convenience APIs unless verified compatible.
- If a release build fails once with ThinLTO / `Broken module found` / `aws_lc_rs` noise, retry the same command once before blaming the patch.
```

## Proposed Ongoing Metrics

Track these after future sessions:

| Metric | Why It Matters |
|---|---|
| Primary task category | Portfolio view of what the team is asking agents to do. |
| Command-weighted effort | Better effort proxy than session count. |
| Validation share | Shows whether time is going into proof or implementation. |
| Error/retry density | Highlights friction hotspots. |
| Live-operation count | Separates real netdisk mutations and live smoke checks from static coding. |
| Commit closure rate | Measures how often sessions end in a clean, reviewable state. |
| Governance/meta overhead | Keeps approval-review rows from polluting engineering metrics. |

Recommended effort formula:

```text
active_work_units =
  weighted_tool_time
  + edit_batches
  + validation_runs
  + live_ops
  + retry_penalty
```

Use idle-clipped duration if timestamps are used; cap long gaps so overnight or resumed sessions do not become fake engineering hours.
