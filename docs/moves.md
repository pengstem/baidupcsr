# Move Log

This log records implementation moves made while completing the Rust + Ratatui rewrite plan.

## 2026-05-17

- Stage 0 started: made planning documents trackable, kept `reference/` ignored as external source material, and began adding compatibility fixtures plus validation records.
- Stage 0 committed as `e71fbae` with tracked plan, compatibility notes, synthetic fixtures, validation template, and move log.
- Stage 1 started: added a dependency-free Rust workspace scaffold with CLI, config, PCS, core, preview, and TUI crates.
- Stage 1 TDD RED: `cargo test --workspace` failed because crate identity functions and CLI `main` were intentionally missing.
- Stage 1 GREEN: added minimal crate identity functions and a `doctor --redact` CLI shell, then verified tests, clippy, formatting, and doctor output.
- Stage 1 committed as `11abbcd` with the Rust workspace baseline.
- Stage 2 started: added failing tests for BaiduPCS-Go config discovery, JSON loading, active user selection, cookie auth presence, and redacted doctor output.
- Stage 2 RED: `cargo test --workspace` failed on missing config APIs, then GREEN after implementing the read-only config model and doctor integration.
- Stage 2 tmux smoke: ran `doctor --redact` in `baidupcsr-stage2`; it loaded the local BaiduPCS-Go config and printed only redacted credential presence.
- Stage 2 committed as `4e1ffa7` with read-only config compatibility and redacted doctor integration.
- Stage 3 started: added failing tests for PCS/Pan response parsing, request URL construction, locatedownload signing, devuid generation, and dlink filtering.
- Stage 3 RED: `cargo test -p baidupcsr-pcs --test stage3_api` failed on missing PCS API core; a wrong devuid expectation was corrected after checking `md5sum`.
- Stage 3 GREEN: implemented fixture-based PCS API core and verified workspace test, clippy, and formatting. Real HTTP execution remains a later step.
- Stage 3 committed as `b40acde` with PCS/Pan API core.
- Stage 4 started: added failing tests for stale-while-revalidate directory cache behavior, generation ordering, change detection, and session path state.
- Stage 4 RED: `cargo test -p baidupcsr-core --test stage4_directory_cache` failed on missing cache/session types.
- Stage 4 GREEN: implemented a pure directory cache state machine and session state, then verified workspace tests, clippy, and formatting.
- Stage 4 committed as `a50717e` with directory cache state machine.
- Stage 5 started: added failing tests for default Vim keymap, TUI mode changes, selection bounds, status line, and preview budget status.
- Stage 5 RED: `cargo test -p baidupcsr-tui --test stage5_tui_state` failed on missing TUI state/keymap types.
- Stage 5 GREEN: implemented the TUI state core and verified workspace tests, clippy, and formatting. Ratatui terminal rendering remains future work.
- Stage 5 committed as `fb9cb19` with TUI state core.
- Stage 6 started: added failing tests for in-memory text preview, cache keys, LRU eviction, and stale request rejection.
- Stage 6 RED: `cargo test -p baidupcsr-preview --test stage6_preview_manager` failed on missing preview manager types; test API was corrected to return `PreviewUpdate`.
- Stage 6 GREEN: implemented an in-memory preview manager with budget enforcement, LRU eviction, and generation-based stale protection.
- Stage 6 committed as `0ff3873` with the in-memory preview manager.
- Stage 7 started: added failing tests for strict video preview session URLs, loopback-only endpoint policy, token/session validation, HEAD/Range proxy decisions, and bounded chunk cache behavior.
- Stage 7 RED: `cargo test -p baidupcsr-preview --test stage7_video_preview` failed on missing video preview types.
- Stage 7 GREEN: implemented strict video preview core types without starting a real HTTP server or external player yet.
- Stage 7 committed as `dbfdfff` with strict video preview core.
- Stage 5b started after feedback: added Ratatui `TestBackend` render tests so the TUI stage covers actual terminal drawing instead of only state transitions.
- Stage 5b RED: `cargo test -p baidupcsr-tui --test stage5b_ratatui_render` failed on missing `render_app`; first implementation exposed a truncated preview budget line, so Preview budget rendering was split into readable fields.
- Stage 5b corrected: removed runtime demo entries; `tui` now reads the active BaiduPCS-Go config and performs a real read-only PCS list before rendering.
- Stage 5b live smoke exposed real response compatibility gaps (`md5` and `errno` omitted on success); fixed parser defaults and added a regression test.
- Stage 5b GREEN: implemented real Ratatui layout, config-backed remote listing, CLI `tui` entrypoint, and tmux smoke with `j/k/q` interaction.

## 2026-05-18

- Stage 5/5b gate tightened after feedback: runtime mock data is forbidden, `l/h/r` real navigation is mandatory, async/caching/UI quality are acceptance criteria, and each complete stage needs tests plus tmux validation.
- Stage 5b yazi calibration: checked yazi `Folder`, `Files`, `Scrollable`, Dispatcher, and Executor structure before extending the browser path.
- Stage 5b RED: added tests for cached refresh preserving stale entries, error refresh preserving cache, directory-first sorting, selected-directory navigation, long-list scrolling, selected-row highlight, and wide-name truncation.
- Stage 5b GREEN: wired `DirectoryCache` into the real CLI runtime, moved PCS list calls into background workers, rejected stale generations, restored parent selections, and prefetches selected directories on `j/k`.
- Stage 5b UI pass: files now render directory-first with truncation, visible selected-row background, responsive pane layout, two-line Tasks area, cache age, request state, and nonblank Preview metadata.
- Stage 5b real tmux smoke: escalated tmux run used the local BaiduPCS-Go config, loaded a real remote list, verified `j/k/l/h/r/q`, confirmed `l` on files does not enter, and recorded redacted evidence in `docs/validation/2026-05-18-stage-5b-real-browser.md`.
- Stage 6 integration started: extended the previous preview-manager foundation into the real runtime path with preview planning, bounded PCS Range reads, TUI text content state, and selection-bound request rejection.
- Stage 6 RED: added tests for preview planning, bounded Range headers/read limits, TUI text-preview state/rendering, CLI preview key mapping, and a focused 80x24 regression where text content hid `disk=false`.
- Stage 6 GREEN: real text preview now renders budget status before content, uses the in-memory `PreviewManager` cache, cooperatively cancels stale preview workers, keeps binary/large files metadata-only, and keeps Preview separate from Enter/Open/Download paths.
- Stage 6 real tmux smoke: escalated TUI run used the local BaiduPCS-Go config, selected a real remote `.txt` file, showed `preview=text`, bounded fetched bytes, `disk=false`, verified quick cursor movement did not show stale text, and found no recent `/tmp` preview files.
- Stage 7 integration started: extended the earlier video model into the real runtime with sidecar kill switch, `p` preview action, loopback HTTP server, controlled mpv launch, and real PCS/Pan locatedownload Range fetching.
- Stage 7 RED: added tests for sidecar video config, video metadata classification, loopback server HEAD/GET/token/cache behavior, Pan UA separation, dlink Range cancellation, optional-errno locatedownload parsing, BDUSS extraction, and mpv launch flags that disable user config/scripts.
- Stage 7 debugging: real tmux smoke first returned loopback `500` because video fetch still used direct path download; after switching to `locatedownload -> dlink -> Range`, a live redacted diagnostic exposed that real successful locatedownload responses can omit `errno`.
- Stage 7 player hardening: a smoke run showed local mpv config spawning a thumb helper and creating `/tmp/thumbfast*`; fixed by adding `--no-config`, `--load-scripts=no`, `--osc=no`, and `--no-terminal`, then reran smoke to confirm no helper/temp files.
- Stage 7 GREEN: TUI video preview now shows `video=loopback seek=true`, loopback listen port, player pid, range/window, cache budget, hit/upstream counts, `disk=false`, and `error=none`.
- Stage 7 real tmux smoke: escalated run used the local BaiduPCS-Go config, selected a real large video, verified `pgrep` showed only a loopback player URL, `ss` showed only `127.0.0.1:<port>`, `curl -r 0-1023` returned `206 Partial Content`, repeated range hit cache, and switching selection removed mpv plus the loopback listener.
- Stage 8 started: split explicit transfer tasks from preview/navigation with a shared `TaskManager`, `d` download action, `x` cancel action, and bottom task summaries.
- Stage 8 RED: added tests for task ids/states/progress/speed/cancel/retry/upload policy, TUI `d`/`x` key state, task panel lines, explicit download target sanitization, dlink candidate fallback, and preserving current selection while refreshing after transfer events.
- Stage 8 hardening: fixed a cancellation/completion race so late completion after a cancel request reaches a terminal state instead of leaving a task stuck in `cancelling`.
- Stage 8 download path: explicit downloads now write to `BAIDUPCSR_DOWNLOAD_DIR`/`savedir`, use `.part` files, resume from partial length, fetch 1 MiB dlink Range chunks, try later locatedownload candidates, and only rename after all bytes are written.
- Stage 8 real tmux smoke: escalated run used the local BaiduPCS-Go config, downloaded a real small file to a smoke directory while `j/k` remained responsive, then cancelled a real large-file download and confirmed a visible `download cancelled` task plus a preserved `.part` file.
- Stage 9 started: added PCS/core/TUI red tests for file-operation requests, dry-run operation plans, multi-select, command input, search, and confirm mode.
- Stage 9 GREEN: implemented `mkdir`, `search`, batch `delete`, `copy`, and `move` request support; command mode now parses selected-entry and explicit-path operations into background remote tasks.
- Stage 9 safety pass: destructive `rm/delete` enters Confirm mode with a dry-run summary and requires `y`; a live 24-line smoke exposed the prompt being hidden, so Tasks rendering now prioritizes the summary plus `y/n` prompt.
- Stage 9 real tmux smoke: escalated run used the local BaiduPCS-Go config, created smoke-owned temporary directories, exercised mkdir/rename/copy/move/search/delete, confirmed delete required `y`, and cleaned the temporary remote directories.
- Stage 9.5 started after UI/cache feedback: inserted a quality gate before Stage 10 for non-video preview coverage, meaningful left context, dense right preview details, persistent cache, paged preload, video diagnostics, and visual polish.
- Stage 9.5 RED: added tests for image/PDF/archive/directory preview planning, PNG header metadata, subtitle text preview, persistent directory cache storage, paged preload limits, default strict video enablement, Context rendering, preview detail density, and navigation events reaching the runtime.
- Stage 9.5 GREEN: implemented `$XDG_CACHE_HOME`/`~/.cache` directory snapshots, paged preload planning, Context panel, richer Preview details, image header metadata, subtitle text previews, visible mpv launch flags, and navigation-triggered preview/preload refresh.
- Stage 9.5 real tmux smoke: used the local BaiduPCS-Go config with a smoke cache dir, verified Context/Preview/preload fields on a real directory, confirmed real subtitle preview updates after `j/k`, triggered strict video preview via `p`, verified mpv only received a loopback URL, and confirmed exit cleaned mpv plus the loopback listener.
- Stage 10 started: added release-readiness RED tests for `doctor` diagnostics, missing-config repair hints, and bounded preload work reporting.
- Stage 10 RED: `cargo test -p baidupcsr-cli --test stage10_doctor_release` failed on missing `ui_config_path` and repair hint; `cargo test -p baidupcsr-core --test stage10_preload_performance` failed because `PagedPreloadPlan` had no `considered_entries`.
- Stage 10 GREEN: `doctor --redact` now reports sidecar config path, cache dir, config permission, video preview state, mpv availability, and repair guidance; `PagedPreloadPlan` now records the bounded window size considered for preload.
- Stage 10 documentation pass: added migration guide, release checklist, bash completion, man page, release profile settings, and redacted validation notes for real and missing-config doctor smoke.
- MPV feedback fix: default video preview now uses the user's mpv config/scripts and a larger bounded 64 MiB cache window; the old no-config/no-scripts behavior remains available as `video_player_profile = "isolated"`.
- Video gateway Task 1 started after playback failure feedback: separated the immediate diagnostic contract from the later buffering rewrite, keeping Preview/Play/Open as distinct modes.
- Video gateway Task 1 RED/GREEN: added playback state and bounded player-log models, redacted URLs plus token-like fields, captured mpv stderr into an in-memory ring, and surfaced `playback=...` plus `player_log=...` in the video preview panel.
- Video gateway Task 1 real tmux smoke: used the local BaiduPCS-Go config, selected a real video, verified the 80x24 TUI shows `playback=playing`, `mpv_profile=user`, and `player_log=none`, confirmed mpv only received a loopback URL, and confirmed exit cleaned up mpv plus the listener.
- Video gateway Task 2 RED/GREEN: added `VideoIntervalBuffer` so a cached interval can serve overlapping subranges, with LRU eviction bounded by the video memory budget.
- Video gateway Task 3 RED/GREEN: split explicit video ranges from requested range shapes, accepted open-ended and suffix HTTP Range headers, and resolved suffix ranges against the remote file tail.
- Video gateway Task 4 RED/GREEN: switched the loopback server to interval buffering, added `VideoGatewayPolicy`, and enabled a background forward prefetch window for CLI video playback without delaying the first response.
