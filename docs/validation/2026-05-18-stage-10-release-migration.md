# Stage 10 Release And Migration Validation

## Metadata

- Date: 2026-05-18
- Stage: 10, polish/release/migration
- Git commit: pending at validation time
- Host: local development machine
- Command surface: `doctor`, core preload planning, docs, packaging metadata
- Sensitive output redacted: yes

## Objective

This stage verifies that the Rust rewrite has a release-readiness path instead
of only implementation internals:

- `doctor --redact` reports config, sidecar, cache, video, player, and config
  permission status.
- Missing config produces a direct repair hint.
- Paged preload reports bounded work for large directories.
- Migration and release docs explain Go/Rust coexistence, rollback, packaging,
  and un-migrated features.

## RED Evidence

These tests were added before implementation and observed failing:

```text
cargo test -p baidupcsr-cli --test stage10_doctor_release
cargo test -p baidupcsr-core --test stage10_preload_performance
```

Expected failures:

- `doctor` output did not contain `ui_config_path:`.
- missing config output did not contain `repair: set BAIDUPCS_GO_CONFIG_DIR`.
- `PagedPreloadPlan` did not expose `considered_entries`.

## Implementation Summary

- `PagedPreloadPlan` now includes `considered_entries`, calculated from the
  bounded visible-page-neighborhood window.
- `doctor --redact` now prints `ui_config_path`, `cache_dir`,
  `config_permission`, `video_preview`, and `mpv`.
- Missing config now prints a repair hint pointing at
  `BAIDUPCS_GO_CONFIG_DIR`.
- The release profile is explicit in the workspace manifest.
- Added migration guide, release checklist, bash completion, and a man page.

## Doctor Smoke

Commands run:

```text
cargo run -p baidupcsr-cli -- doctor --redact
cargo run -p baidupcsr-cli -- doctor --redact --config-dir /tmp/baidupcsr-stage10-missing-config
```

tmux release smoke session:

```text
baidupcsr-stage10-release
target/release/baidupcsr-cli doctor --redact
target/release/baidupcsr-cli doctor --redact --config-dir /tmp/baidupcsr-stage10-missing-config
```

Redacted observations:

- Real local config was found and loaded.
- `config_permission` reported an owner-only config mode.
- `ui_config_path`, `cache_dir`, `video_preview`, and `mpv` were present.
- Credentials were printed only as presence labels.
- Missing config reported `config: not loaded`, `config_permission: missing`,
  and the repair hint.
- The tmux session was killed after capture.

The real output contained a private remote workdir and is not copied here.

## Verification

Commands run before commit:

```text
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p baidupcsr-cli
bash -n completions/baidupcsr.bash
git diff --check
```

Results:

- Formatting check passed.
- Workspace tests passed, including the new Stage 10 doctor and preload tests.
- Clippy passed with `-D warnings`.
- Release build produced `target/release/baidupcsr-cli`.
- Bash completion passed syntax validation.
- Whitespace check passed.

## Conclusion

Stage 10 has a test-backed release-readiness path and documentation for
migration, rollback, terminal validation, packaging, and remaining feature gaps.
This does not claim full BaiduPCS-Go parity; it documents the coexistence
boundary explicitly.
