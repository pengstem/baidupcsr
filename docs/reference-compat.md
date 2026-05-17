# Reference Compatibility Notes

These notes capture the compatibility contract taken from the Go reference implementation in `reference/`.

## Config Discovery

The Rust implementation must read the existing BaiduPCS-Go configuration without requiring a new login.

Lookup order:

1. `BAIDUPCS_GO_CONFIG_DIR` when set.
2. Existing `pcs_config.json` next to the executable.
3. Windows: `%APPDATA%\BaiduPCS-Go\pcs_config.json`.
4. Unix-like systems: `$HOME/.config/BaiduPCS-Go/pcs_config.json`.

The first Rust implementation should read the Go config as immutable input. Rust-specific UI state must live in a sidecar config under the same config root or an app-specific Rust config directory.

## Required Config Fields

- Account list: `baidu_active_uid`, `baidu_user_list`.
- User identity and auth: `uid`, `name`, `bduss`, `ptoken`, `stoken`, `baiduid`, `sboxtkn`, `cookies`, `accesstoken`, `workdir`.
- Network behavior: `appid`, `user_agent`, `pcs_ua`, `pcs_addr`, `pan_ua`, `enable_https`, `proxy`, `proxy_hostnames`, `local_addrs`.
- Transfer tuning: `cache_size`, `max_parallel`, `max_download_load`, `max_download_rate`, `max_upload_parallel`, `max_upload_load`, `max_upload_rate`.
- Behavior switches: `no_check`, `savedir`, `ignore_illegal`, `u_policy`, `fix_pcs_addr`, `force_login_username`.

Unknown fields must be preserved in memory where practical and ignored safely.

## Remote File Model

The Rust file model must represent at least these response fields:

- `fs_id`
- `app_id`
- `path`
- `server_filename`
- `ctime`
- `mtime`
- `md5`
- `block_list`
- `size`
- `isdir`
- `ifhassubdir`

Preview cache keys should prefer account identity plus `fs_id`, `mtime`, and `size` over path-only keys.

## Download And Preview Paths

The Go reference has these relevant paths:

- PCS file download: `/rest/2.0/pcs/file?method=download`.
- PCS stream download: `/rest/2.0/pcs/stream?method=download`.
- Pan/locatedownload dlink flow using `time`, `rand`, `devuid`, `cuid`, Cookie, and Pan UA.

The Rust design must keep these actions separate:

- Preview: bounded memory, no implicit disk write, cancellable.
- Open: explicit user intent, may use external tools, must explain any temporary file behavior.
- Download: explicit disk write through the task system.

Video playback must not pass the real dlink to a player. It must use a local bounded streaming proxy before playback is enabled.

