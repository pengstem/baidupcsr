use baidupcsr_pcs::FileEntry;
use baidupcsr_tui::{render_app, PreviewBudget, PreviewKind, TuiApp};
use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};
use std::convert::Infallible;

fn file(fs_id: i64, path: &str, size: i64, is_dir: bool) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: path.to_string(),
        server_filename: path.trim_start_matches('/').to_string(),
        ctime: 1,
        mtime: 1,
        md5: String::new(),
        block_list: Vec::new(),
        size,
        is_dir,
        has_subdir: is_dir,
    }
}

fn file_with_name(fs_id: i64, path: &str, name: &str, size: i64, is_dir: bool) -> FileEntry {
    FileEntry {
        server_filename: name.to_string(),
        ..file(fs_id, path, size, is_dir)
    }
}

fn file_with_mtime(fs_id: i64, path: &str, size: i64, mtime: i64) -> FileEntry {
    FileEntry {
        mtime,
        ..file(fs_id, path, size, false)
    }
}

fn draw_buffer(app: &TuiApp, preview: Option<PreviewBudget>, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = infallible(Terminal::new(backend));
    infallible(terminal.draw(|frame| render_app(frame, app, preview)));

    terminal.backend().buffer().clone()
}

fn draw_to_text(app: &TuiApp, preview: Option<PreviewBudget>) -> String {
    let buffer = draw_buffer(app, preview, 96, 24);
    buffer_to_text(&buffer)
}

fn buffer_to_text(buffer: &Buffer) -> String {
    let mut output = String::new();
    for y in buffer.area.top()..buffer.area.bottom() {
        for x in buffer.area.left()..buffer.area.right() {
            output.push_str(buffer[(x, y)].symbol());
        }
        output.push('\n');
    }
    output
}

fn line_with_text_has_bg(buffer: &Buffer, text: &str, color: Color) -> bool {
    for y in buffer.area.top()..buffer.area.bottom() {
        let mut line = String::new();
        for x in buffer.area.left()..buffer.area.right() {
            line.push_str(buffer[(x, y)].symbol());
        }
        if line.contains(text) {
            for x in buffer.area.left()..buffer.area.right() {
                if buffer[(x, y)].bg == color {
                    return true;
                }
            }
        }
    }

    false
}

fn infallible<T>(result: Result<T, Infallible>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => match err {},
    }
}

#[test]
fn ratatui_render_draws_three_panes_and_preview_budget() {
    let mut app = TuiApp::new("/apps");
    app.set_entries(vec![
        file(1, "/apps", 0, true),
        file(2, "/apps/demo.txt", 128, false),
    ]);
    app.set_background_tasks(1);

    let screen = draw_to_text(
        &app,
        Some(PreviewBudget {
            kind: PreviewKind::Text,
            fetched_bytes: 128,
            memory_limit_bytes: 16 * 1024 * 1024,
            truncated: false,
            disk_backed: false,
        }),
    );

    assert!(screen.contains("baidupcsr"));
    assert!(!screen.contains("Context"));
    assert!(screen.contains("Files"));
    assert!(screen.contains("Preview"));
    assert!(screen.contains("> [ ] apps/"));
    assert!(screen.contains("demo.txt"));
    assert!(screen.contains("Preview: text"));
    assert!(!screen.contains("limit 16777216"));
    assert!(!screen.contains("Truncated: false"));
    assert!(!screen.contains("disk false"));
    assert!(!screen.contains("mode=NORMAL"));
    assert!(screen.contains("1/2"));
    assert!(screen.contains("0 selected"));
    assert!(screen.contains("1 task"));
}

#[test]
fn ratatui_render_draws_path_as_breadcrumb_blocks() {
    let app = TuiApp::new("/docs/videos");

    let screen = draw_to_text(&app, None);

    assert!(screen.contains(" /  >  docs  >  videos "));
}

#[test]
fn ratatui_render_keeps_selection_visible_after_navigation() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![
        file(1, "/alpha", 0, true),
        file(2, "/beta.txt", 512, false),
    ]);
    app.handle_key(baidupcsr_tui::KeyEvent::Char('j'));

    let screen = draw_to_text(&app, None);

    assert!(screen.contains("  [ ] alpha/"));
    assert!(screen.contains("> [ ] beta.txt"));
    assert!(screen.contains("metadata"));
}

#[test]
fn ratatui_render_scrolls_long_lists_and_highlights_selected_row() {
    let mut app = TuiApp::new("/");
    let entries: Vec<FileEntry> = (0..30)
        .map(|index| file(index, &format!("/item-{index:02}.txt"), 512, false))
        .collect();
    app.set_entries(entries);

    for _ in 0..18 {
        app.handle_key(baidupcsr_tui::KeyEvent::Char('j'));
    }

    let buffer = draw_buffer(&app, None, 72, 16);
    let screen = buffer_to_text(&buffer);

    assert!(screen.contains("item-18.txt"));
    assert!(!screen.contains("item-00.txt"));
    assert!(line_with_text_has_bg(
        &buffer,
        "item-18.txt",
        Color::Rgb(14, 116, 144)
    ));
    assert!(screen.contains("Files 18-"));
    assert!(screen.contains("/30"));
}

#[test]
fn ratatui_render_keeps_cursor_and_mark_columns_separate() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![
        file(1, "/a.txt", 512, false),
        file(2, "/b.txt", 512, false),
    ]);

    app.handle_key(baidupcsr_tui::KeyEvent::Char(' '));
    let selected_marked = draw_to_text(&app, None);
    assert!(selected_marked.contains("> [x] a.txt"));

    app.handle_key(baidupcsr_tui::KeyEvent::Char('j'));
    let buffer = draw_buffer(&app, None, 80, 16);
    let screen = buffer_to_text(&buffer);

    assert!(screen.contains("  [x] a.txt"));
    assert!(screen.contains("> [ ] b.txt"));
    assert!(line_with_text_has_bg(
        &buffer,
        "[x] a.txt",
        Color::Rgb(30, 41, 59)
    ));
}

#[test]
fn ratatui_render_truncates_long_wide_names_without_breaking_layout() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file_with_name(
        1,
        "/docs/一个非常非常非常非常非常长的中文文件名-with-a-long-tail.txt",
        "一个非常非常非常非常非常长的中文文件名-with-a-long-tail.txt",
        2048,
        false,
    )]);

    let screen = buffer_to_text(&draw_buffer(&app, None, 64, 16));

    assert!(screen.contains("一 个"));
    assert!(screen.contains("..."));
    assert!(screen.contains("with-a-long-tail.txt"));
    assert!(screen.contains("Preview"));
    assert!(screen.contains("1/1"));
}

#[test]
fn ratatui_render_draws_text_preview_content() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/notes.md", 11, false)]);
    app.finish_preview_text(
        "notes.md",
        "hello\nworld",
        PreviewBudget {
            kind: PreviewKind::Text,
            fetched_bytes: 11,
            memory_limit_bytes: 16 * 1024 * 1024,
            truncated: false,
            disk_backed: false,
        },
    );

    let screen = draw_to_text(&app, None);

    assert!(screen.contains("hello"));
    assert!(screen.contains("world"));
    assert!(screen.contains("Preview: text"));
    assert!(screen.contains("11 B fetched"));
    assert!(screen.contains("memory only"));
    assert!(!screen.contains("16777216"));
    assert!(!screen.contains("disk false"));
}

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
    assert!(help.contains("gg/G"));
    assert!(help.contains("Esc"));
    assert!(help.contains("y/n"));
    assert!(help.contains("Mouse"));
    assert!(help.contains("path click jumps"));
}

#[test]
fn ratatui_render_keeps_video_playback_diagnostics_visible() {
    let mut app = TuiApp::new("/videos");
    app.set_entries(vec![file(1, "/videos/movie.mp4", 2048, false)]);
    app.finish_preview_video(
        "movie.mp4",
        vec![
            "video=loopback seek=true".to_string(),
            "playback=playing".to_string(),
            "mpv_profile=user".to_string(),
            "player_log=none".to_string(),
            "listen=127.0.0.1:42843".to_string(),
        ],
        PreviewBudget {
            kind: PreviewKind::Video,
            fetched_bytes: 1024,
            memory_limit_bytes: 64 * 1024 * 1024,
            truncated: false,
            disk_backed: false,
        },
    );

    let screen = buffer_to_text(&draw_buffer(&app, None, 80, 24));

    assert!(screen.contains("playback=playing"));
    assert!(screen.contains("mpv_profile=user"));
    assert!(screen.contains("player_log=none"));
}

#[test]
fn ratatui_render_keeps_preview_budget_visible_with_text_content() {
    let mut app = TuiApp::new("/docs/course/chapter/session");
    app.set_entries(vec![file(
        1,
        "/docs/course/chapter/session/readme.txt",
        96,
        false,
    )]);
    app.finish_preview_text(
        "readme.txt",
        "line one\nline two\nline three\nline four\nline five\nline six\nline seven",
        PreviewBudget {
            kind: PreviewKind::Text,
            fetched_bytes: 96,
            memory_limit_bytes: 16 * 1024 * 1024,
            truncated: false,
            disk_backed: false,
        },
    );

    let screen = buffer_to_text(&draw_buffer(&app, None, 80, 24));

    assert!(screen.contains("Preview: text"));
    assert!(screen.contains("96 B fetched"));
    assert!(screen.contains("memory only"));
    assert!(screen.contains("line one"));
}

#[test]
fn ratatui_render_formats_large_sizes_and_timestamps_for_people() {
    let mut app = TuiApp::new("/videos");
    app.set_entries(vec![
        file_with_mtime(
            1,
            "/videos/clip.mkv",
            5_i64 * 1024 * 1024 * 1024,
            1_700_000_000,
        ),
        file(2, "/videos/sample.bin", 1_992_294, false),
    ]);

    let screen = draw_to_text(&app, None);

    assert!(screen.contains("5.0 GiB"));
    assert!(screen.contains("1.9 MiB"));
    assert!(screen.contains("2023-11-14 22:13"));
    assert!(!screen.contains("5120MiB"));
    assert!(!screen.contains("1700000000"));
}

#[test]
fn ratatui_render_distinguishes_loading_empty_and_error_file_states() {
    let mut loading = TuiApp::new("/docs");
    loading.begin_directory_load("/docs");
    assert!(draw_to_text(&loading, None).contains("Loading..."));

    let empty = TuiApp::new("/empty");
    assert!(draw_to_text(&empty, None).contains("Empty directory"));

    let mut failed = TuiApp::new("/missing");
    failed.finish_directory_error("/missing", Vec::new(), None, "remote_list_error: 403", None);
    let screen = draw_to_text(&failed, None);
    assert!(screen.contains("Error: remote_list_error: 403"));
}

#[test]
fn ratatui_render_shows_filter_input_in_bottom_modeline() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![
        file(1, "/notes.md", 1024, false),
        file(2, "/photo.jpg", 2048, false),
    ]);
    app.handle_key(baidupcsr_tui::KeyEvent::Char('/'));
    app.handle_key(baidupcsr_tui::KeyEvent::Char('n'));

    let screen = draw_to_text(&app, None);
    let last_line = screen.lines().last().unwrap_or_default();

    assert!(last_line.contains("/n|"));
    assert!(screen.contains("Files 1-1/1 of 2"));
    assert!(screen.contains("notes.md"));
    assert!(!screen.contains("photo.jpg"));
}

#[test]
fn ratatui_render_marks_truncated_preview_and_task_lists() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/notes.md", 128, false)]);
    app.finish_preview_text(
        "notes.md",
        "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12",
        PreviewBudget {
            kind: PreviewKind::Text,
            fetched_bytes: 128,
            memory_limit_bytes: 16 * 1024 * 1024,
            truncated: true,
            disk_backed: false,
        },
    );
    app.set_preload_lines(vec![
        "queued dirs=1 previews=1".to_string(),
        "window=0..10".to_string(),
        "extra=1".to_string(),
    ]);

    let tall = buffer_to_text(&draw_buffer(&app, None, 96, 30));
    assert!(tall.contains("+3 more"));

    let normal = draw_to_text(&app, None);
    assert!(normal.contains("+2 more"));
}

#[test]
fn ratatui_render_shows_destructive_confirm_as_centered_modal() {
    let mut app = TuiApp::new("/docs");
    app.set_task_lines(vec!["#7 remote done 0/? search".to_string()]);
    app.begin_confirm(vec![
        "delete 2 remote entries".to_string(),
        "/docs/a.txt".to_string(),
        "press y to confirm, n to cancel".to_string(),
    ]);

    let screen = draw_to_text(&app, None);

    assert!(screen.contains("Confirm"));
    assert!(screen.contains("delete 2 remote entries"));
    assert!(screen.contains("/docs/a.txt"));
    assert!(screen.contains("[y] confirm"));
    assert!(screen.contains("[n] cancel"));
    assert!(!screen.contains("press y to confirm"));
}
