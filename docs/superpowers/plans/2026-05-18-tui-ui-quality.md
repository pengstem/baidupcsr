# TUI UI Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve the Ratatui browser UI with a focused two-pane layout, hidden help overlay, arrow/mouse input parity, and broader preview classification.

**Architecture:** Keep the existing `baidupcsr-tui` state/render boundary and CLI runtime event handler. Add small input/layout helpers inside `crates/baidupcsr-tui/src/lib.rs`, keep preview classification in `crates/baidupcsr-preview/src/lib.rs`, and map new preview kinds in `crates/baidupcsr-cli/src/main.rs`.

**Tech Stack:** Rust, Ratatui 0.30, Crossterm 0.29 via Ratatui re-export, current workspace test suite, tmux live validation.

---

### Task 1: Hidden Help Overlay And Arrow Key Parity

**Files:**
- Modify: `crates/baidupcsr-tui/src/lib.rs`
- Modify: `crates/baidupcsr-tui/tests/stage5_tui_state.rs`
- Modify: `crates/baidupcsr-tui/tests/stage5b_ratatui_render.rs`

- [ ] **Step 1: Write failing state tests**

Add tests asserting `?` toggles help, help is not visible by default, and arrow-key `KeyEvent` variants match Vim actions:

```rust
#[test]
fn question_mark_toggles_hidden_help_overlay() {
    let mut app = TuiApp::new("/docs");

    assert!(!app.help_visible());
    assert_eq!(app.handle_key_event(KeyEvent::Char('?')), None);
    assert!(app.help_visible());
    assert_eq!(app.handle_key_event(KeyEvent::Esc), None);
    assert!(!app.help_visible());
}

#[test]
fn arrow_keys_mirror_vim_navigation_actions() {
    assert_eq!(DEFAULT_KEYMAP.action_for(KeyEvent::Down), KeyAction::Down);
    assert_eq!(DEFAULT_KEYMAP.action_for(KeyEvent::Up), KeyAction::Up);
    assert_eq!(DEFAULT_KEYMAP.action_for(KeyEvent::Left), KeyAction::Back);
    assert_eq!(DEFAULT_KEYMAP.action_for(KeyEvent::Right), KeyAction::Enter);
}
```

- [ ] **Step 2: Write failing render test**

Add a render test proving the normal screen does not visibly advertise help, but `?` opens a compact overlay:

```rust
#[test]
fn help_overlay_is_hidden_until_question_mark() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/readme.md", 1024, false)]);

    let normal = draw_to_text(&app, None);
    assert!(!normal.contains("Shortcuts"));
    assert!(!normal.contains("Mouse"));

    app.handle_key(baidupcsr_tui::KeyEvent::Char('?'));
    let help = draw_to_text(&app, None);
    assert!(help.contains("Shortcuts"));
    assert!(help.contains("j/k"));
    assert!(help.contains("Mouse"));
}
```

- [ ] **Step 3: Run tests to verify RED**

Run:

```bash
cargo test -p baidupcsr-tui --test stage5_tui_state question_mark_toggles_hidden_help_overlay
cargo test -p baidupcsr-tui --test stage5_tui_state arrow_keys_mirror_vim_navigation_actions
cargo test -p baidupcsr-tui --test stage5b_ratatui_render help_overlay_is_hidden_until_question_mark
```

Expected: fail because `help_visible`, arrow `KeyEvent` variants, and overlay rendering do not exist.

- [ ] **Step 4: Implement minimal state and render support**

Add `Up`, `Down`, `Left`, `Right` to `KeyEvent`; map them in `KeyMap`; add `help_visible: bool` to `TuiApp`; add `help_visible()`; make `?` toggle and `Esc` close help. Render a centered `Shortcuts` overlay when `help_visible` is true.

- [ ] **Step 5: Run GREEN tests**

Run the three focused commands from Step 3. Expected: pass.

### Task 2: Mouse Input And Terminal Mouse Capture

**Files:**
- Modify: `crates/baidupcsr-tui/src/lib.rs`
- Modify: `crates/baidupcsr-tui/tests/stage5_tui_state.rs`

- [ ] **Step 1: Write failing mouse state tests**

Add public mouse input types and tests:

```rust
#[test]
fn mouse_click_selects_visible_file_row() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![
        file(1, "/docs/a.txt"),
        file(2, "/docs/b.txt"),
        file(3, "/docs/c.txt"),
    ]);

    let event = app.handle_mouse_event(baidupcsr_tui::MouseInput {
        kind: baidupcsr_tui::MouseInputKind::LeftClick,
        column: 4,
        row: 7,
        area: ratatui::layout::Rect::new(0, 0, 96, 24),
    });

    assert_eq!(app.selected_index(), Some(1));
    assert_eq!(event, Some(TuiRuntimeEvent::Action(KeyAction::Select)));
}

#[test]
fn mouse_wheel_moves_selection_like_keyboard_navigation() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/a.txt"), file(2, "/docs/b.txt")]);

    assert_eq!(
        app.handle_mouse_event(baidupcsr_tui::MouseInput {
            kind: baidupcsr_tui::MouseInputKind::ScrollDown,
            column: 4,
            row: 7,
            area: ratatui::layout::Rect::new(0, 0, 96, 24),
        }),
        Some(TuiRuntimeEvent::Action(KeyAction::Down))
    );
    assert_eq!(app.selected_index(), Some(1));
}
```

- [ ] **Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p baidupcsr-tui --test stage5_tui_state mouse_click_selects_visible_file_row
cargo test -p baidupcsr-tui --test stage5_tui_state mouse_wheel_moves_selection_like_keyboard_navigation
```

Expected: fail because mouse input types and `KeyAction::Select` do not exist.

- [ ] **Step 3: Implement mouse mapping**

Add:

```rust
pub enum MouseInputKind { LeftClick, DoubleClick, ScrollUp, ScrollDown }
pub struct MouseInput { pub kind: MouseInputKind, pub column: u16, pub row: u16, pub area: Rect }
```

Expose a layout helper used by both render and mouse mapping. File clicks inside the files pane select the clicked visible row and return `Action(Select)`. Double click on a directory returns `Action(Enter)`. Wheel events call `move_up`/`move_down` and return `Action(Up)`/`Action(Down)`.

- [ ] **Step 4: Enable terminal mouse capture**

In `run_terminal_app_with_handler`, enable Crossterm mouse capture after `ratatui::init()` and disable it before `ratatui::restore()`. Extend the event loop to map `CrosstermEvent::Mouse` into `MouseInput`.

- [ ] **Step 5: Run GREEN tests**

Run the two focused mouse tests. Expected: pass.

### Task 3: Focused Browser Layout And Preview Readability

**Files:**
- Modify: `crates/baidupcsr-tui/src/lib.rs`
- Modify: `crates/baidupcsr-tui/tests/stage5b_ratatui_render.rs`
- Modify: `crates/baidupcsr-tui/tests/stage9_5_tui_quality.rs`

- [ ] **Step 1: Write failing render tests**

Add/update tests proving the wide layout is two-pane focused, not a vague context rail:

```rust
#[test]
fn focused_browser_wide_layout_removes_context_rail() {
    let mut app = TuiApp::new("/remote/current");
    app.set_entries(vec![
        file(1, "/remote/current/photos", 0, true),
        file(2, "/remote/current/readme.md", 2048, false),
    ]);

    let screen = draw(&app, 108, 24);

    assert!(screen.contains("/remote/current"));
    assert!(screen.contains("Files"));
    assert!(screen.contains("Preview"));
    assert!(!screen.contains("Context"));
    assert!(!screen.contains("Locations"));
    assert!(!screen.contains("keys j/k"));
}
```

Add a suffix-preservation preview test:

```rust
#[test]
fn preview_panel_wraps_long_name_without_losing_suffix() {
    let mut app = TuiApp::new("/videos");
    app.set_entries(vec![file(
        1,
        "/videos/A.Knight.of.the.Seven.Kingdoms.S01E01.2160p.MAX.WEB-DL.mkv",
        5_830 * 1024 * 1024,
        false,
    )]);

    let screen = draw(&app, 108, 24);

    assert!(screen.contains("A.Knight"));
    assert!(screen.contains(".mkv"));
    assert!(!screen.contains("name=A.Knight"));
}
```

- [ ] **Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p baidupcsr-tui --test stage9_5_tui_quality focused_browser_wide_layout_removes_context_rail
cargo test -p baidupcsr-tui --test stage9_5_tui_quality preview_panel_wraps_long_name_without_losing_suffix
```

Expected: fail because the current wide layout renders `Context`, long preview values use `...`, and the status line contains long key hints.

- [ ] **Step 3: Implement layout and copy changes**

Change wide layout to two panes: files and preview. Move useful context into `header_widget` and `tasks_widget`. Rewrite `preview_widget` detail lines to human labels:

```text
Name: ...
Kind: file · video
Size: 5.8 GiB
Modified: ...
Path: ...
Preview: video · memory only
```

Keep raw `key=value` diagnostic lines for video playback lines and errors only. Shorten `status_widget` to mode/path/request/cache/tasks without long key-hint prose.

- [ ] **Step 4: Run GREEN tests**

Run the two focused render tests plus existing render suite:

```bash
cargo test -p baidupcsr-tui --test stage5b_ratatui_render
cargo test -p baidupcsr-tui --test stage9_5_tui_quality
```

Expected: pass after updating older assertions to the new human-readable preview copy.

### Task 4: Broader Preview Classification

**Files:**
- Modify: `crates/baidupcsr-preview/src/lib.rs`
- Modify: `crates/baidupcsr-preview/tests/stage9_5_preview_coverage.rs`
- Modify: `crates/baidupcsr-cli/src/main.rs`
- Modify: `crates/baidupcsr-cli/src/main.rs` unit tests near preview kind mapping
- Modify: `crates/baidupcsr-tui/src/lib.rs`

- [ ] **Step 1: Write failing preview planner tests**

Add tests:

```rust
#[test]
fn preview_plan_classifies_audio_documents_ebooks_and_fonts() {
    let audio = PreviewPlan::for_entry("album.flac", false, 4096, 16 * 1024 * 1024);
    let office = PreviewPlan::for_entry("report.docx", false, 4096, 16 * 1024 * 1024);
    let ebook = PreviewPlan::for_entry("book.epub", false, 4096, 16 * 1024 * 1024);
    let font = PreviewPlan::for_entry("font.woff2", false, 4096, 16 * 1024 * 1024);

    assert_eq!(audio.kind, PreviewKind::Audio);
    assert_eq!(audio.action, PreviewAction::MetadataOnly);
    assert_eq!(office.kind, PreviewKind::Document);
    assert_eq!(office.action, PreviewAction::MetadataOnly);
    assert_eq!(ebook.kind, PreviewKind::Ebook);
    assert_eq!(ebook.action, PreviewAction::MetadataOnly);
    assert_eq!(font.kind, PreviewKind::Font);
    assert_eq!(font.action, PreviewAction::MetadataOnly);
}

#[test]
fn preview_plan_treats_checksums_torrents_and_playlists_as_text_when_small() {
    let checksum = PreviewPlan::for_entry("SHA256SUMS", false, 512, 16 * 1024 * 1024);
    let torrent = PreviewPlan::for_entry("linux.iso.torrent", false, 4096, 16 * 1024 * 1024);
    let playlist = PreviewPlan::for_entry("playlist.m3u8", false, 4096, 16 * 1024 * 1024);

    assert_eq!(checksum.kind, PreviewKind::Text);
    assert_eq!(checksum.action, PreviewAction::FetchText);
    assert_eq!(torrent.kind, PreviewKind::Text);
    assert_eq!(torrent.action, PreviewAction::FetchText);
    assert_eq!(playlist.kind, PreviewKind::Text);
    assert_eq!(playlist.action, PreviewAction::FetchText);
}
```

- [ ] **Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p baidupcsr-preview --test stage9_5_preview_coverage preview_plan_classifies_audio_documents_ebooks_and_fonts
cargo test -p baidupcsr-preview --test stage9_5_preview_coverage preview_plan_treats_checksums_torrents_and_playlists_as_text_when_small
```

Expected: fail because the new preview kinds are not defined and some extensions classify as metadata.

- [ ] **Step 3: Implement preview kind expansion**

Add `Audio`, `Document`, `Ebook`, and `Font` to preview kind enums in preview/TUI crates and update CLI mapping. Extend `preview_kind_from_name` for audio, Office, e-book, font, torrent, playlist, checksum, and common source/config file names.

- [ ] **Step 4: Run GREEN tests**

Run:

```bash
cargo test -p baidupcsr-preview --test stage9_5_preview_coverage
cargo test -p baidupcsr-cli preview_kind_mapping_preserves_all_preview_boundaries
```

Expected: pass after mapping new kinds.

### Task 5: Documentation, Live Validation, And Commit

**Files:**
- Modify: `docs/moves.md`
- Create: `docs/validation/2026-05-18-tui-ui-quality.md`

- [ ] **Step 1: Update docs with implementation evidence**

Record the Focused Browser direction, `?` hidden help behavior, arrow/mouse input support, and preview type coverage.

- [ ] **Step 2: Run full automated gate**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Expected: all pass.

- [ ] **Step 3: Run real tmux TUI validation**

Run release or dev TUI in tmux using the local BaiduPCS-Go config:

```bash
tmux new-session -d -s baidupcsr-ui-quality 'cargo run -p baidupcsr-cli -- tui'
tmux send-keys -t baidupcsr-ui-quality '?'
tmux capture-pane -pt baidupcsr-ui-quality
tmux send-keys -t baidupcsr-ui-quality Escape
tmux send-keys -t baidupcsr-ui-quality Down Up Right Left
tmux capture-pane -pt baidupcsr-ui-quality
tmux send-keys -t baidupcsr-ui-quality q
tmux kill-session -t baidupcsr-ui-quality
```

Expected: real list loads, help overlay appears only after `?`, arrows navigate, right/left enter/back where applicable, and the Preview panel shows readable labels without `name=...` truncation.

- [ ] **Step 4: Commit**

Run:

```bash
git add crates/baidupcsr-tui/src/lib.rs crates/baidupcsr-tui/tests/stage5_tui_state.rs crates/baidupcsr-tui/tests/stage5b_ratatui_render.rs crates/baidupcsr-tui/tests/stage9_5_tui_quality.rs crates/baidupcsr-preview/src/lib.rs crates/baidupcsr-preview/tests/stage9_5_preview_coverage.rs crates/baidupcsr-cli/src/main.rs docs/moves.md docs/validation/2026-05-18-tui-ui-quality.md docs/superpowers/plans/2026-05-18-tui-ui-quality.md
git commit -m "feat: polish tui browser ui"
```

Expected: one commit containing implementation, tests, docs, and the implementation plan.

---

## Self-Review

- Spec coverage: hidden `?` help, arrows, mouse, Focused Browser layout, preview coverage, visual cleanup, automated tests, and tmux validation are all mapped to tasks.
- Placeholder scan: no TBD/TODO placeholders remain.
- Type consistency: new planned public names are `KeyEvent::{Up,Down,Left,Right}`, `KeyAction::Select`, `MouseInputKind`, `MouseInput`, and preview kinds `Audio`, `Document`, `Ebook`, `Font`.
