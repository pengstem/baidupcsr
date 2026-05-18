# Video Playback Gateway Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the brittle passive video preview path with a diagnosable local media gateway contract that can grow into smooth strict playback.

**Architecture:** Keep Preview, strict Play, and future Open separate. The first implementation slice adds explicit playback status, player diagnostics, and mode/profile reporting without changing the upstream security boundary. Later slices replace exact-offset chunk caching with interval buffering and prefetch.

**Tech Stack:** Rust 2021, existing `baidupcsr-preview`, `baidupcsr-cli`, `baidupcsr-config`, Ratatui state rendering, `cargo test`, `cargo clippy`.

---

## File Map

- Modify `crates/baidupcsr-preview/src/lib.rs`: playback status model, player stderr ring, sanitized player diagnostics, and the Task 2 interval buffer.
- Modify `crates/baidupcsr-preview/tests/stage7_video_preview.rs`: RED/GREEN coverage for playback status, profile args, diagnostics, and gateway behavior.
- Modify `crates/baidupcsr-config/src/lib.rs`: sidecar config for prefetch/stall/log settings.
- Modify `crates/baidupcsr-config/tests/stage2_config.rs`: config parsing tests.
- Modify `crates/baidupcsr-cli/src/main.rs`: player stderr capture, status mapping, TUI preview lines, and future gateway prefetch wiring.
- Modify `crates/baidupcsr-tui/src/lib.rs` and tests only if rendering truncates the new diagnostics.
- Add or update `docs/validation/YYYY-MM-DD-video-playback-gateway.md`: red/green/live-smoke record.
- Update `docs/moves.md`: implementation moves.

## Task 1: Playback Mode And Diagnostics Contract

**Files:**
- Modify: `crates/baidupcsr-preview/src/lib.rs`
- Modify: `crates/baidupcsr-preview/tests/stage7_video_preview.rs`
- Modify: `crates/baidupcsr-cli/src/main.rs`
- Modify: `docs/validation/2026-05-18-video-playback-gateway.md`

- [x] **Step 1: Write failing preview tests**

Add tests asserting:

```rust
#[test]
fn player_log_ring_keeps_recent_sanitized_lines() {
    let mut log = PlayerLogRing::new(2);
    log.push("Opening http://127.0.0.1:1234/preview/video/tok_secret");
    log.push("A: 00:00:01 / 00:20:00");
    log.push("Error: https://d.pcs.baidu.com/file/private?token=SECRET");

    assert_eq!(log.lines().len(), 2);
    assert!(log.lines()[0].contains("A: 00:00:01"));
    assert!(log.lines()[1].contains("<redacted-url>"));
    assert!(!log.lines()[1].contains("SECRET"));
}

#[test]
fn playback_status_marks_stalled_after_no_progress_timeout() {
    let status = PlaybackProgress::new(PlaybackState::Playing)
        .with_last_activity_ms(1_000)
        .status_at(7_000, 5_000);

    assert_eq!(status, PlaybackState::Stalled);
}
```

- [x] **Step 2: Run RED**

Run: `cargo test -p baidupcsr-preview --test stage7_video_preview player_log_ring_keeps_recent_sanitized_lines playback_status_marks_stalled_after_no_progress_timeout`

Expected: compile failure for missing `PlayerLogRing`, `PlaybackProgress`, and `PlaybackState`.

- [x] **Step 3: Implement minimal preview types**

Add:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackState {
    Preparing,
    Buffering,
    Playing,
    Stalled,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackProgress {
    state: PlaybackState,
    last_activity_ms: u64,
}

impl PlaybackProgress {
    pub const fn new(state: PlaybackState) -> Self {
        Self { state, last_activity_ms: 0 }
    }

    pub const fn with_last_activity_ms(mut self, last_activity_ms: u64) -> Self {
        self.last_activity_ms = last_activity_ms;
        self
    }

    pub const fn status_at(&self, now_ms: u64, stall_timeout_ms: u64) -> PlaybackState {
        if matches!(self.state, PlaybackState::Playing)
            && now_ms.saturating_sub(self.last_activity_ms) >= stall_timeout_ms
        {
            PlaybackState::Stalled
        } else {
            self.state
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerLogRing {
    limit: usize,
    lines: VecDeque<String>,
}
```

Implement `new`, `push`, `lines`, and URL redaction for `http://`, `https://`, token-looking substrings.

- [x] **Step 4: Run GREEN**

Run: `cargo test -p baidupcsr-preview --test stage7_video_preview`

Expected: all stage7 preview tests pass.

- [x] **Step 5: Wire CLI display**

Extend active video preview lines to include:

```text
playback=playing
mpv_profile=user
player_log=<last sanitized line or none>
```

Keep this minimal; do not yet change buffering behavior.

- [x] **Step 6: Verify**

Run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both pass.

- [x] **Step 7: Commit**

```bash
git add crates/baidupcsr-preview/src/lib.rs crates/baidupcsr-preview/tests/stage7_video_preview.rs crates/baidupcsr-cli/src/main.rs docs/validation/2026-05-18-video-playback-gateway.md docs/moves.md
git commit -m "feat: add video playback diagnostics contract"
```

## Task 2: Interval Buffer Contract

**Files:**
- Modify: `crates/baidupcsr-preview/src/lib.rs`
- Modify: `crates/baidupcsr-preview/tests/stage7_video_preview.rs`

- [x] **Step 1: Write failing tests**

Add tests:

```rust
#[test]
fn interval_buffer_serves_overlapping_subranges() -> Result<(), VideoPreviewError> {
    let budget = VideoCacheBudget::new(16, 2)?;
    let mut buffer = VideoIntervalBuffer::new(budget);
    buffer.put(VideoRange::new(0, 15)?, b"abcdefghijklmnop".to_vec())?;

    assert_eq!(buffer.get(VideoRange::new(4, 7)?), Some(b"efgh".to_vec()));
    assert_eq!(buffer.get(VideoRange::new(12, 20)?), None);
    Ok(())
}

#[test]
fn interval_buffer_evicts_until_within_budget() -> Result<(), VideoPreviewError> {
    let budget = VideoCacheBudget::new(8, 2)?;
    let mut buffer = VideoIntervalBuffer::new(budget);
    buffer.put(VideoRange::new(0, 7)?, b"aaaaaaaa".to_vec())?;
    buffer.put(VideoRange::new(8, 15)?, b"bbbbbbbb".to_vec())?;
    buffer.put(VideoRange::new(16, 23)?, b"cccccccc".to_vec())?;

    assert_eq!(buffer.used_bytes(), 16);
    assert!(buffer.get(VideoRange::new(0, 7)?).is_none());
    assert_eq!(buffer.get(VideoRange::new(16, 23)?), Some(b"cccccccc".to_vec()));
    Ok(())
}
```

- [x] **Step 2: Run RED**

Run: `cargo test -p baidupcsr-preview --test stage7_video_preview interval_buffer`

Expected: compile failure for missing `VideoIntervalBuffer`.

- [x] **Step 3: Implement minimal interval buffer**

Add `VideoIntervalBuffer` alongside `VideoChunkCache`, leaving existing server behavior untouched until Task 3. Use a small internal `CachedInterval { range, bytes, last_used }` vector and evict oldest intervals while `used_bytes > memory_limit_bytes`.

- [x] **Step 4: Run GREEN**

Run: `cargo test -p baidupcsr-preview --test stage7_video_preview interval_buffer`

Expected: the two interval tests pass.

- [x] **Step 5: Commit**

```bash
git add crates/baidupcsr-preview/src/lib.rs crates/baidupcsr-preview/tests/stage7_video_preview.rs
git commit -m "feat: add video interval buffer"
```

## Task 3: Gateway Request Semantics

**Files:**
- Modify: `crates/baidupcsr-preview/src/lib.rs`
- Modify: `crates/baidupcsr-preview/tests/stage7_video_preview.rs`

- [ ] **Step 1: Write failing tests**

Add tests for request parsing:

```rust
#[test]
fn range_parser_accepts_open_ended_and_suffix_ranges() -> Result<(), VideoPreviewError> {
    assert_eq!(
        parse_range_header("Range: bytes=4096-").transpose()?,
        Some(VideoRange::new(4096, u64::MAX)?)
    );
    assert_eq!(
        parse_range_header("Range: bytes=-8192").transpose()?,
        Some(VideoRange::suffix(8192)?)
    );
    Ok(())
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p baidupcsr-preview --test stage7_video_preview range_parser_accepts_open_ended_and_suffix_ranges`

Expected: suffix range fails because current parser requires a numeric start.

- [ ] **Step 3: Implement suffix range model**

Extend `VideoRange` with a suffix representation or add a `RequestedVideoRange` enum:

```rust
pub enum RequestedVideoRange {
    Explicit(VideoRange),
    Suffix(u64),
}
```

Convert suffix ranges to explicit ranges once file size is known.

- [ ] **Step 4: Run GREEN**

Run: `cargo test -p baidupcsr-preview --test stage7_video_preview`

Expected: all preview tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/baidupcsr-preview/src/lib.rs crates/baidupcsr-preview/tests/stage7_video_preview.rs
git commit -m "feat: support mpv range request shapes"
```

## Task 4: Prefetching Gateway

**Files:**
- Modify: `crates/baidupcsr-preview/src/lib.rs`
- Modify: `crates/baidupcsr-preview/tests/stage7_video_preview.rs`
- Modify: `crates/baidupcsr-cli/src/main.rs`

- [ ] **Step 1: Write failing tests**

Add a loopback server test where the first request fetches the requested range
and the next two chunks:

```rust
#[test]
fn loopback_server_prefetches_forward_window_without_exceeding_budget() -> Result<(), String> {
    let calls = Arc::new(AtomicUsize::new(0));
    let server = VideoLoopbackServer::spawn_with_policy(
        key(),
        VideoUpstream::new("https://example.invalid/video").map_err(|err| format!("{err:?}"))?,
        VideoToken::new("tok_0123456789abcdef").map_err(|err| format!("{err:?}"))?,
        VideoCacheBudget::new(1024, 4).map_err(|err| format!("{err:?}"))?,
        VideoGatewayPolicy { prefetch_chunks: 2 },
        {
            let calls = Arc::clone(&calls);
            move |range| {
                calls.fetch_add(1, Ordering::AcqRel);
                let len = range.end_inclusive.saturating_sub(range.start).saturating_add(1);
                Ok(vec![b'x'; usize::try_from(len).unwrap_or(0)])
            }
        },
    ).map_err(|err| format!("{err:?}"))?;

    let _ = http_get(server.endpoint().port(), server.player_url(), Some("bytes=0-1023"))?;
    assert!(calls.load(Ordering::Acquire) >= 2);
    assert!(server.stats().used_bytes <= 4096);
    Ok(())
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p baidupcsr-preview --test stage7_video_preview loopback_server_prefetches_forward_window_without_exceeding_budget`

Expected: compile failure for missing `spawn_with_policy` and `VideoGatewayPolicy`.

- [ ] **Step 3: Implement policy**

Add:

```rust
pub struct VideoGatewayPolicy {
    pub prefetch_chunks: usize,
}
```

After serving the requested range, synchronously prefetch the next N chunk-sized
intervals for the first implementation. Keep it simple and bounded; do not add a
background prefetch worker in this task.

- [ ] **Step 4: Run GREEN**

Run: `cargo test -p baidupcsr-preview --test stage7_video_preview loopback_server_prefetches_forward_window_without_exceeding_budget`

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add crates/baidupcsr-preview/src/lib.rs crates/baidupcsr-preview/tests/stage7_video_preview.rs crates/baidupcsr-cli/src/main.rs
git commit -m "feat: prefetch video gateway window"
```

## Task 5: Live Smoke And Tuning

**Files:**
- Create: `docs/validation/2026-05-18-video-playback-gateway.md`
- Modify: `docs/moves.md`

- [ ] **Step 1: Run static verification**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Expected: all pass.

- [ ] **Step 2: Run tmux smoke**

Run in a dedicated tmux session:

```bash
tmux new-session -d -s baidupcsr-video-gateway -c /home/nastem/Project/baidupcsr
tmux send-keys -t baidupcsr-video-gateway 'cargo run -p baidupcsr-cli -- tui' C-m
```

Select a real video, press `p`, then capture:

```bash
tmux capture-pane -p -S -120 -t baidupcsr-video-gateway
pgrep -af 'mpv|baidupcsr-cli tui|target/debug/baidupcsr-cli'
ss -ltnp
```

Expected:

- mpv command contains only loopback URL, no dlink;
- TUI shows `playback`, `mpv_profile`, `range`, `buffer`, and diagnostics;
- gateway requests progress beyond the initial range or reports a clear stalled
  state with player/proxy details;
- no raw remote path, token, or dlink is written to docs.

- [ ] **Step 3: Write validation record**

Add a redacted summary to `docs/validation/2026-05-18-video-playback-gateway.md`.

- [ ] **Step 4: Commit**

```bash
git add docs/validation/2026-05-18-video-playback-gateway.md docs/moves.md
git commit -m "docs: validate video playback gateway"
```

## Self-Review

- Spec coverage: Tasks cover diagnostics, status, interval buffer, Range
  semantics, prefetch policy, and live smoke.
- Placeholder scan: no TBD/TODO placeholders.
- Scope check: this is one subsystem, split into independently committable
  slices.
- Type consistency: names are stable across tasks:
  `PlaybackState`, `PlaybackProgress`, `PlayerLogRing`,
  `VideoIntervalBuffer`, `VideoGatewayPolicy`.
