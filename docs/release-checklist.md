# Release Checklist

Stage 10 release readiness is a gate, not a claim of full BaiduPCS-Go parity.

## Build And Test

Run before tagging or handing a binary to users:

```bash
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
git diff --check
```

The release profile uses LTO, one codegen unit, and stripped binaries.

## Doctor Gate

Run:

```bash
cargo run -- doctor --redact
```

Verify the output includes:

- `config_path`
- `ui_config_path`
- `cache_dir`
- `config_permission`
- `video_preview`
- `mpv`
- `config: loaded`
- active UID and credential presence only, never raw token values

The `video_preview` line should also expose the player profile and cache budget,
for example `profile=user chunk=4194304 chunks=32 budget=134217728`.

If config loading fails, `doctor` must print a repair hint that starts with
`repair: set BAIDUPCS_GO_CONFIG_DIR`.

## Terminal Matrix

Each release candidate should have a short validation note for:

- local Linux terminal;
- tmux session;
- SSH session;
- narrow 80x24 terminal;
- one wider terminal where the Preview pane has enough space.

macOS and Windows are release targets only after a maintainer records equivalent
terminal validation. Until then they are compatibility goals, not verified
platforms.

## Performance Smoke

Required checks:

- `cargo test --lib paged_preload_records_bounded_work_for_large_directories`
- real TUI rapid `j/k` movement in a populated directory;
- warm start with a populated cache directory;
- refresh while moving selection;
- text preview followed by video metadata or strict video preview.

The preload plan must stay bounded to the visible page neighborhood. It must not
scan or enqueue an entire large directory just because the directory is large.

For video smoothness, the default profile should allow user mpv config/scripts
and use a larger bounded cache window. If a release candidate reverts to
`profile=isolated` or a tiny video budget, record why.

## Security Smoke

Before release, inspect outputs from `doctor`, TUI status lines, validation
records, and panic/error paths for these forbidden values:

- raw Cookie;
- BDUSS;
- STOKEN;
- SBOXTKN;
- access token;
- dlink;
- private video token.

`doctor` may print paths and active UID, but credential values must be reduced
to presence labels. Validation docs must redact private remote paths and
filenames when they come from a real account.

The default mpv profile is user-friendly rather than isolated. It still hides
the upstream dlink from mpv, but local mpv scripts are trusted code and may have
their own side effects. Use `video_player_profile = "isolated"` when the release
smoke needs the older no-config/no-scripts guarantee.

## Packaging Artifacts

Build:

```bash
cargo build --release
```

Installable local artifacts:

- binary: `target/release/baidupcsr-cli`;
- bash completion: `completions/baidupcsr.bash`;
- man page: `docs/man/baidupcsr-cli.1`.

Static Linux builds are expected to be feasible because HTTP uses rustls, but
they are only considered verified after:

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

Record the result in `docs/validation/` before claiming static packaging
support.
