# Stage 7 Video Integration Validation

## Metadata

- Date: 2026-05-18
- Stage: 7 integration
- Status: complete
- Git commit: stage 7 integration commit (see `git log`)
- Host: local development workspace
- Command surface: cargo, tmux, ss, pgrep, curl
- Sensitive output redacted: yes

## Objective

Complete the strict video preview stage beyond the earlier pure model: video preview must use a local loopback proxy, never expose a Baidu dlink to the player, keep chunk cache memory bounded, avoid disk preview/cache files, and invalidate the active session when selection changes.

## Implemented Design

- Video preview remains opt-in through the sidecar UI config. The default config keeps playback disabled and shows metadata only.
- Pressing `p` on a video entry starts a `VideoLoopbackServer` bound to `127.0.0.1:0` with a random token URL.
- The player launch receives only `http://127.0.0.1:<port>/preview/video/<token>`. It never receives the upstream dlink.
- The CLI fetcher lazily calls PCS `locatedownload`, caches the selected dlink only inside the process, then serves player `Range` requests by fetching bounded dlink ranges.
- The loopback server supports `HEAD`, `GET`, `Range`, `Accept-Ranges`, `Content-Range`, token validation, cache hit stats, and fixed-size in-memory chunk caching.
- The controlled mpv backend disables user config/scripts and disk cache with `--no-config`, `--load-scripts=no`, `--cache-on-disk=no`, `--demuxer-max-bytes=<budget>`, and related null-output/headless flags.
- The TUI video panel shows loopback mode, seek capability, listen port, player pid or exit status, last requested range, cache window, cache usage, hit/upstream counts, `disk=false`, and last error.
- Switching selection, entering another directory, or leaving the directory drops `ActiveVideoPreview`, kills/waits the player, shuts down the loopback server, and increments a cancellation epoch used by in-flight fetches.

## TDD Evidence

RED examples:

```bash
cargo test -p baidupcsr-pcs --test stage3_api
cargo test -p baidupcsr-preview --test stage7_video_preview video_session_exposes_only_loopback_token_url_to_player
cargo test -p baidupcsr-preview --test stage7_video_preview loopback_server_reuses_chunk_cache_and_reports_stats
cargo test -p baidupcsr-cli
```

Observed failures covered:

- Missing `PcsApi::locatedownload_request`.
- Missing separate Pan UA in `BlockingPcsClientOptions`.
- Missing arbitrary dlink `download_url_range_with_cancel`.
- Real `locatedownload` success responses omitting `errno`.
- Player launch not disabling local mpv config/scripts, which allowed a local thumb helper to create `/tmp/thumbfast*` files during an early smoke.
- Video proxy stats missing last range, cache window, and last error.

GREEN evidence:

```bash
cargo test -p baidupcsr-pcs --test stage3_api
cargo test -p baidupcsr-preview --test stage7_video_preview
cargo test -p baidupcsr-cli
cargo test -p baidupcsr-cli live_video_dlink_range_smoke_is_redacted -- --ignored --nocapture
```

Observed result:

- PCS API tests passed, including locatedownload request construction, optional-errno parsing, Pan UA separation, and dlink Range cancellation.
- Preview tests passed, including loopback-only URL policy, token rejection, HEAD/Range responses, chunk cache budget, cache hit stats, and controlled mpv launch flags.
- CLI tests passed; the ignored live diagnostic used the machine-local config, printed only dlink count and byte count, and fetched 1024 bytes from a real dlink Range without printing URL/path/token/Cookie.

## Real tmux Smoke

Session:

- `baidupcsr-stage7-video-rerun`

Redacted sequence:

1. Started `cargo run -p baidupcsr-cli -- tui` in tmux with `BAIDUPCSR_UI_CONFIG` pointing at the Stage 7 sidecar fixture that enables strict video preview.
2. Loaded a real remote directory from the local BaiduPCS-Go active config.
3. Selected a real large video entry and pressed `p`.
4. TUI showed `preview=video`, `fetched<=524288`, `limit=524288`, `disk=false`, `video=loopback seek=true`, a loopback listen port, mpv pid, `range=<start>-<end>`, `window=<start>-<end>`, cache usage, hit/upstream counts, and `error=none`.
5. `pgrep -af mpv` showed mpv launched with `--no-config --load-scripts=no --cache-on-disk=no --demuxer-max-bytes=524288` and only a redacted loopback URL.
6. `ss -ltnp` showed the preview server listening only on `127.0.0.1:<port>`.
7. `curl -r 0-1023 -D - -o /dev/null http://127.0.0.1:<port>/preview/video/<token>` returned `HTTP/1.1 206 Partial Content`, `Accept-Ranges: bytes`, `Content-Range: bytes 0-1023/<size>`, and `Content-Length: 1024`.
8. Repeating the same Range request returned 206 and updated the cache-hit path.
9. Moving the selection to another video dropped active playback: `pgrep -af mpv` found no mpv/helper process, `ss -ltnp` no longer showed the old loopback port, and the TUI returned to metadata-only video preview.
10. Checked for the earlier mpv-script artifact pattern under `/tmp/thumbfast*`; no files were created after adding `--no-config` and `--load-scripts=no`.
11. Pressed `q`; the tmux validation session exited.

Raw pane captures included private remote names, local token values, and real file sizes. They were inspected only for status fields and are intentionally not stored here.

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
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.

## Redaction

No Cookie, BDUSS, STOKEN, access token, dlink, raw remote path, raw preview content, or loopback token is written here.

## Conclusion

Stage 7 integration passes the strict video preview gate: the player sees only a local token URL, the proxy is loopback-only and Range-capable, the chunk cache stays within the configured memory budget, mpv is prevented from loading user scripts or disk cache, switching selection cancels the old session, and the final smoke did not create preview/temp files.
