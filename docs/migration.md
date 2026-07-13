# Migration Guide

This project is designed to coexist with BaiduPCS-Go instead of replacing its
login state.

## Quick Start

1. Keep your existing BaiduPCS-Go login and `pcs_config.json`.
2. Run `cargo run -- doctor --redact`.
3. Check `config: loaded`, `active_uid`, credential presence, `cache_dir`,
   `ui_config_path`, `video_preview`, `mpv`, and `config_permission`.
4. Run `cargo run -- tui`.

If the config is not found, set:

```bash
export BAIDUPCS_GO_CONFIG_DIR="$HOME/.config/BaiduPCS-Go"
```

Then rerun `doctor --redact`.

## What Is Reused

- `pcs_config.json` is read from the BaiduPCS-Go-compatible discovery path.
- The active user is selected from `baidu_active_uid`.
- Existing BDUSS/STOKEN/cookie fields are reused for authenticated requests.
- The active user's `workdir` becomes the initial TUI path.
- Download defaults can still use the Go config `savedir`, unless
  `BAIDUPCSR_DOWNLOAD_DIR` is set.

The Rust code treats the Go config as read-only. It does not rewrite
`pcs_config.json`.

## Rust-Specific State

Rust-only state is kept outside `pcs_config.json`:

- UI sidecar: `BAIDUPCSR_UI_CONFIG`, otherwise
  `$HOME/.config/baidupcsr/ui_config.toml`.
- Directory cache: `BAIDUPCSR_CACHE_DIR`, otherwise
  `$XDG_CACHE_HOME/baidupcsr` or `$HOME/.cache/baidupcsr`.

Video preview sidecar keys:

```toml
video_preview_enabled = true
video_chunk_size_bytes = 4194304
video_max_chunks = 32
video_player_profile = "user"
```

`video_player_profile = "user"` is the default. It lets mpv load the user's
normal config, OSD style, and scripts while still receiving only the local
loopback URL and keeping `--cache-on-disk=no`. Use `"isolated"` to restore the
stricter no-config/no-scripts launch profile for smoke tests or when a local mpv
script writes unwanted temp files.

The directory cache stores remote directory metadata. If filenames or paths are
sensitive on the local machine, point `BAIDUPCSR_CACHE_DIR` at an encrypted or
temporary location. Removing this directory is safe; the TUI will reload from
the remote API.

## Coexistence Boundary

Use BaiduPCS-Go for account login, config repair, and any feature that is not
yet migrated. Use baidupcsr for the Rust TUI browser, preview, strict video
preview, explicit download tasks, search, and basic remote file operations.

The current Rust side does not claim full parity with BaiduPCS-Go. Notable gaps:

- sharing, transfer-save, recycle-bin workflows, and offline download;
- high-fidelity Office/PDF rendering;
- full terminal image protocols beyond bounded image metadata;
- broad upload workflows;
- every possible video container or non-Range playback case.

## Rollback

Rollback is simple because the Go config is read-only:

1. Exit the TUI with `q`.
2. Stop any running preview player if the terminal was killed abruptly.
3. Optionally remove the Rust sidecar config and cache directories.
4. Continue using BaiduPCS-Go with the same `pcs_config.json`.

No BaiduPCS-Go login migration is required in either direction.
