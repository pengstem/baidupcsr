# Move Log

This log records implementation moves made while completing the Rust + Ratatui rewrite plan.

## 2026-05-17

- Stage 0 started: made planning documents trackable, kept `reference/` ignored as external source material, and began adding compatibility fixtures plus validation records.
- Stage 0 committed as `e71fbae` with tracked plan, compatibility notes, synthetic fixtures, validation template, and move log.
- Stage 1 started: added a dependency-free Rust workspace scaffold with CLI, config, PCS, core, preview, and TUI crates.
- Stage 1 TDD RED: `cargo test --workspace` failed because crate identity functions and CLI `main` were intentionally missing.
- Stage 1 GREEN: added minimal crate identity functions and a `doctor --redact` CLI shell, then verified tests, clippy, formatting, and doctor output.

