# Stage 9.5 Quality, Cache, and Preview Validation

## Scope

This stage responds to the UI/cache/preview quality gate added after Stage 9. It blocks Stage 10 until the real TUI stops feeling like a debug panel and covers these product requirements:

- Preview coverage beyond video: directory, text/subtitle, image metadata, PDF/archive metadata, video metadata/playback.
- Left pane has real context instead of placeholder Locations.
- Right Preview pane uses space for selected-item details, preview budget, and useful content.
- Directory cache persists under the user cache directory and is reused before refresh.
- Preload is paged and bounded instead of scanning whole directories.
- Video preview is diagnostic and visible: default strict preview is enabled after explicit `p`, mpv uses a loopback URL, and the launch no longer uses null video output.

## Yazi Calibration

The implementation follows the yazi patterns already noted in the plan:

- `Folder` keeps cursor, offset, page, and revision/ticket-style state so navigation and async results are separate.
- Yazi preloaders are paged: work is scoped to the visible page neighborhood rather than the whole directory.
- Preview/preload work is discardable, so fast navigation does not let stale work own the UI.

Sources used for calibration:

- https://github.com/sxyazi/yazi/blob/main/yazi-core/src/tab/folder.rs
- https://github.com/sxyazi/yazi/issues/143

## RED Evidence

New tests were written before implementation and observed failing for missing behavior:

```text
cargo test -p baidupcsr-preview --test stage9_5_preview_coverage
cargo test -p baidupcsr-core --test stage9_5_cache_preload
cargo test -p baidupcsr-tui --test stage9_5_tui_quality
cargo test -p baidupcsr-config --test stage9_5_video_defaults
```

Observed failures covered missing image metadata parsing, missing preview kinds/actions, missing persistent cache/preload APIs, missing `CacheSource`/Context TUI state, default video preview still disabled, and navigation not emitting runtime events for preview refresh.

## Implementation Summary

- `PreviewPlan` now distinguishes directory, text, image, PDF, archive, metadata, and video.
- PNG/JPEG header metadata is parsed from bounded bytes without disk files.
- `.srt`, `.ass`, and `.vtt` subtitle files use the text preview path.
- Directory snapshots are written to the configured cache root using stable per-account/path filenames.
- Runtime startup and directory changes hydrate memory cache from disk before background refresh.
- A paged preload planner queues only nearby directory and lightweight preview targets.
- The left pane is `Context`: parent, current path, selected item, marked count, cache source, cache age, and preload window.
- The right Preview pane always shows selected-item details and budget before content.
- Default video preview is enabled for explicit `p`; mpv launch uses `--force-window=yes`, keeps disk cache disabled, and uses only the loopback URL.

## Real tmux Smoke

Session:

```text
baidupcsr-stage9-5-quality
```

Environment:

```text
BAIDUPCSR_CACHE_DIR=/tmp/baidupcsr-stage9-5-cache
```

Redacted steps and observations:

1. Removed the smoke cache directory and launched `target/debug/baidupcsr-cli tui` in tmux with the local BaiduPCS-Go config.
2. The TUI loaded a real remote directory, rendered `Context`, `Files`, `Preview`, `Tasks`, and status panes, and did not fall back to mock data.
3. Context showed a real parent/current/selected summary, `cache=network`, cache age, and a bounded preload window.
4. The smoke cache directory received per-directory JSON cache files.
5. Restarted with the same cache directory; refresh completed quickly, and unit coverage verifies the disk snapshot round trip used by this path.
6. Moved the cursor to a real subtitle entry. Preview changed with the selected row and showed `preview=text`, bounded fetched bytes, `disk=false`, and text content.
7. Moved to a real video entry and pressed `p`. Preview showed `video=loopback seek=true`, a `127.0.0.1:<port>` listener, mpv pid, range/window, fixed cache usage, `disk=false`, and `error=none`.
8. `pgrep -af mpv` showed mpv launched with `--no-config`, `--load-scripts=no`, `--cache-on-disk=no`, `--force-window=yes`, and only a redacted loopback URL.
9. `ss -ltnp` showed the preview server listening only on `127.0.0.1:<port>`.
10. Exited with `q`; follow-up `pgrep` found no `mpv` or `baidupcsr-cli tui` process, so the player and loopback listener were cleaned up.

No raw remote path, private filename, Cookie, BDUSS, STOKEN, access token, dlink, or video token is written here.

## Verification

Commands run:

```text
cargo test --workspace
cargo test -p baidupcsr-cli live_video_dlink_range_smoke_is_redacted -- --ignored --nocapture
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Results:

- Workspace tests passed, including Stage 9.5 preview/cache/TUI/config tests.
- The ignored live video range smoke passed and printed only URL count and byte count.
- Formatting, clippy, and whitespace checks passed before writing this validation file.

## Conclusion

Stage 9.5 passes the quality gate for the issues raised after Stage 9: preview coverage is broader, the panes carry real product information, persistent cache and paged preload exist, video preview has a visible strict mpv path, and navigation now refreshes preview state instead of leaving stale content.
