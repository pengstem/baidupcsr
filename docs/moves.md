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
