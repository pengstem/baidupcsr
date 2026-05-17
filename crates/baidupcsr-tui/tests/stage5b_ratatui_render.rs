use baidupcsr_pcs::FileEntry;
use baidupcsr_tui::{render_app, PreviewBudget, PreviewKind, TuiApp};
use ratatui::{backend::TestBackend, Terminal};
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

fn draw_to_text(app: &TuiApp, preview: Option<PreviewBudget>) -> String {
    let backend = TestBackend::new(96, 24);
    let mut terminal = infallible(Terminal::new(backend));
    infallible(terminal.draw(|frame| render_app(frame, app, preview)));

    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for y in buffer.area.top()..buffer.area.bottom() {
        for x in buffer.area.left()..buffer.area.right() {
            output.push_str(buffer[(x, y)].symbol());
        }
        output.push('\n');
    }
    output
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
    assert!(screen.contains("mode=NORMAL path=/apps tasks=1"));
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
