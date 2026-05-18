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
    assert!(screen.contains("Locations"));
    assert!(screen.contains("Files"));
    assert!(screen.contains("Preview"));
    assert!(screen.contains("> apps/"));
    assert!(screen.contains("demo.txt"));
    assert!(screen.contains("preview=text"));
    assert!(screen.contains("fetched=128"));
    assert!(screen.contains("limit=16777216"));
    assert!(screen.contains("truncated=false"));
    assert!(screen.contains("disk=false"));
    assert!(screen.contains("mode=NORMAL path=/apps request=idle cache_age=none tasks=1"));
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

    assert!(screen.contains("  alpha/"));
    assert!(screen.contains("> beta.txt"));
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
    assert!(line_with_text_has_bg(&buffer, "item-18.txt", Color::Blue));
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
    assert!(!screen.contains("with-a-long-tail.txt"));
    assert!(screen.contains("Preview"));
    assert!(screen.contains("Status"));
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
    assert!(screen.contains("preview=text"));
    assert!(screen.contains("disk=false"));
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

    assert!(screen.contains("preview=text"));
    assert!(screen.contains("fetched=96"));
    assert!(screen.contains("disk=false"));
    assert!(screen.contains("line one"));
}

#[test]
fn ratatui_render_prioritizes_confirm_prompt_in_tasks_panel() {
    let mut app = TuiApp::new("/docs");
    app.set_task_lines(vec!["#7 remote done 0/? search".to_string()]);
    app.begin_confirm(vec![
        "delete 2 remote entries".to_string(),
        "/docs/a.txt".to_string(),
        "press y to confirm, n to cancel".to_string(),
    ]);

    let screen = draw_to_text(&app, None);

    assert!(screen.contains("delete 2 remote entries"));
    assert!(screen.contains("press y to confirm"));
}
