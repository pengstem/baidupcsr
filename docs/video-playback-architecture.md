# Video Playback Architecture

## Problem

The current video path started as a strict preview proof: mpv only receives a
local loopback URL, the app controls upstream Range requests, and no preview data
is intentionally written to disk. That security boundary is useful, but the
current implementation is too small to behave like a real player pipeline:

- the loopback server is passive and only fetches the exact Range requested by
  mpv;
- cache hits require an exact chunk offset;
- there is no media/container probe before launching mpv;
- player stderr is discarded, so the TUI cannot explain why playback stopped;
- preview metadata, strict playback, and external open behavior are blurred.

The fix is not another mpv flag. Video needs a real playback architecture with
explicit modes, buffering policy, diagnostics, and fallback behavior.

## Goals

- Keep Preview, Play, and Open as separate user actions.
- Preserve the strict default that mpv never sees the upstream Baidu dlink.
- Make Play behave like playback, not a single bounded preview read.
- Expose enough diagnostics to distinguish player, proxy, network, auth, dlink,
  container, and cache problems.
- Keep memory bounded and observable.
- Allow a user-friendly mpv profile that loads the user's normal mpv style and
  plugins, while retaining an isolated profile for smoke tests.

## Non-Goals

- No direct dlink handoff in Preview or strict Play.
- No hidden full-file download in Preview.
- No claim that every remote video container is playable through Range.
- No attempt to sandbox arbitrary user mpv scripts in `profile=user`; they are
  trusted local user code.

## User Actions

### Preview

Preview is automatic on selection. For videos it should show:

- type, size, mtime, fs_id, path;
- whether the file looks playable through Range;
- detected container hints when available;
- available actions: `p` strict play, `o` external open when implemented, `d`
  download.

Preview may fetch small bounded head/tail samples for probing. It must not start
mpv.

### Play

Play is explicit via `p`. It starts the local media gateway and a player adapter.
The default player adapter is `mpv-user`:

- mpv may load the user's config, style, OSD, and scripts;
- mpv still receives only `http://127.0.0.1:<port>/preview/video/<token>`;
- disk cache remains disabled with `--cache-on-disk=no`.

The `mpv-isolated` adapter is kept for tests and safety-sensitive runs:

- disables user config and scripts;
- useful when local scripts create temp files or interfere with smoke tests.

### Open

Open is a future explicit action for compatibility. It may choose a less strict
backend, but the UI must label the trade-off before launching it. Open is not
Preview and must not be used silently to make Preview "work".

## Components

### MediaProbe

Responsibility: classify the selected remote video before Play.

Inputs:

- `FileEntry`;
- bounded head sample;
- optional bounded tail sample.

Outputs:

- `ProbeStatus::Playable`;
- `ProbeStatus::NeedsTailIndex`;
- `ProbeStatus::UnsupportedContainer`;
- `ProbeStatus::Unknown`;
- human-readable reason.

The first implementation can be conservative: MP4/MOV/MKV/WebM extension plus
Range-capable source is `Unknown` or `Playable`; tail-index handling can be
added once the gateway has diagnostics.

### MediaGateway

Responsibility: serve a local HTTP media endpoint to mpv while controlling all
upstream access.

Required behavior:

- bind only to `127.0.0.1`;
- validate a random token for every request;
- support `HEAD`, bounded `GET`, open-ended `Range`, and suffix `Range`;
- cache by byte interval, not exact offset only;
- answer overlapping requests from cached intervals when possible;
- fetch upstream dlink ranges through the app PCS/Pan client;
- refresh dlink once on 403/expired-link-like failure;
- keep a bounded forward window;
- expose stats to TUI.

The gateway is allowed to prefetch ahead of mpv. It is not allowed to write
video bytes to disk in strict Play mode.

### PlaybackBuffer

Responsibility: hold bounded in-memory byte intervals.

Policy:

- fixed memory limit from sidecar config;
- interval-based lookup;
- LRU or window-based eviction;
- seek clears or narrows the old forward window;
- cache accounting is always visible in the Preview panel.

The current exact-offset `VideoChunkCache` should evolve into this interval
buffer instead of being patched with more special cases.

### PlayerAdapter

Responsibility: launch and monitor a player process.

Profiles:

- `mpv-user`: user config/scripts allowed, local loopback URL only, disk cache
  off, useful cache/readahead flags.
- `mpv-isolated`: no config, no scripts, no OSC, disk cache off, smoke-safe.

The adapter must capture stderr into a bounded in-memory ring buffer. The TUI
must show the last few sanitized player lines when playback is unhealthy.

### PlaybackSession

Responsibility: own a single selected-file playback lifecycle.

State:

- `Preparing`: probe and dlink setup.
- `Buffering`: gateway exists, waiting for useful buffer.
- `Playing`: player running and gateway requests are moving.
- `Stalled`: player alive but no forward progress for a threshold.
- `Failed`: player exit, proxy error, auth error, or unsupported media.
- `Stopped`: user navigated away, changed selection, or quit.

Switching selection, entering another directory, or quitting must stop the
session, kill/wait the player, close the gateway, and cancel upstream work.

## Data Flow

```text
TUI selection
  -> Preview planner
  -> MediaProbe(head/tail samples)
  -> user presses p
  -> PlaybackSession::Preparing
  -> MediaGateway binds 127.0.0.1 and creates token URL
  -> PlayerAdapter launches mpv with local URL
  -> mpv HEAD/GET/Range
  -> MediaGateway checks PlaybackBuffer
  -> missing intervals fetched via PCS/Pan dlink Range
  -> MediaGateway prefetches forward window
  -> stats and player stderr ring flow back to TUI
```

## TUI Diagnostics

The Preview pane should show:

- `playback=<preparing|buffering|playing|stalled|failed|stopped>`;
- `profile=<mpv-user|mpv-isolated>`;
- `listen=127.0.0.1:<port>`;
- `player=mpv pid=<pid>` or `player_exit=<status>`;
- `range=<last request>`;
- `buffer=<start>-<end> used=<bytes>/<limit>`;
- `upstream=<fetches> hits=<hits> refresh=<dlink refresh count>`;
- `last_error=<reason>`;
- last sanitized player line when present.

If mpv is alive but no new Range request or upstream fetch happens for several
seconds while the session has not reached end-of-file, mark it `stalled`.

## Sidecar Config

Recommended keys:

```toml
video_preview_enabled = true
video_chunk_size_bytes = 8388608
video_max_chunks = 16
video_player_profile = "user"
video_prefetch_chunks = 4
video_stall_timeout_ms = 5000
video_player_log_lines = 6
```

Existing keys remain valid. Missing new keys use conservative defaults.

## Testing Strategy

Unit tests:

- player profile command args;
- interval buffer overlapping reads and eviction;
- Range parser for open-ended and suffix requests;
- stalled-session detection;
- stderr ring redaction and line limit;
- dlink refresh retry decision.

Integration tests:

- loopback server serves overlapping mpv-like Range requests from cache;
- gateway prefetches forward intervals without exceeding memory budget;
- switching file cancels old session and closes listener.

Live smoke:

- tmux TUI with real config;
- select real video and press `p`;
- verify mpv receives only loopback URL;
- verify TUI shows profile, buffer window, request progress, and no raw dlink;
- if playback stalls, capture player/proxy diagnostics instead of guessing.

## Rollout

1. Diagnostics and mode contract.
2. Interval buffer and Range parser.
3. Gateway prefetch loop.
4. Player stderr ring and stalled-session UI.
5. Live smoke against real videos and tuning.

Each step must keep the strict invariant that mpv never receives the Baidu dlink
in Preview or strict Play mode.
