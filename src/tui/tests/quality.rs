use super::super::*;
use crate::pcs::FileEntry;
use crate::preview::{PreviewKind, PreviewStatus};
use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};
use std::convert::Infallible;

fn file(fs_id: i64, path: &str, size: i64, is_dir: bool) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: path.to_string(),
        server_filename: path
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(path)
            .to_string(),
        ctime: 1,
        mtime: 1700000000,
        md5: String::new(),
        block_list: Vec::new(),
        size,
        is_dir,
        has_subdir: is_dir,
    }
}

fn draw(app: &TuiApp, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = infallible(Terminal::new(backend));
    infallible(terminal.draw(|frame| render_app(frame, app, None)));
    buffer_to_text(terminal.backend().buffer())
}

fn draw_buffer(app: &TuiApp, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = infallible(Terminal::new(backend));
    infallible(terminal.draw(|frame| render_app(frame, app, None)));
    terminal.backend().buffer().clone()
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

fn text_has_style(
    buffer: &Buffer,
    text: &str,
    expected_fg: Option<Color>,
    expected_bg: Option<Color>,
) -> bool {
    for y in buffer.area.top()..buffer.area.bottom() {
        let mut line = String::new();
        for x in buffer.area.left()..buffer.area.right() {
            line.push_str(buffer[(x, y)].symbol());
        }
        if !line.contains(text) {
            continue;
        }

        for x in buffer.area.left()..buffer.area.right() {
            let cell = &buffer[(x, y)];
            let fg_matches = expected_fg.is_none_or(|fg| cell.fg == fg);
            let bg_matches = expected_bg.is_none_or(|bg| cell.bg == bg);
            if fg_matches && bg_matches {
                return true;
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
fn wide_layout_uses_focused_browser_instead_of_context_rail() {
    let mut app = TuiApp::new("/remote/current");
    app.set_entries(vec![
        file(1, "/remote/current/photos", 0, true),
        file(2, "/remote/current/readme.md", 2048, false),
    ]);
    app.set_cache_source(CacheSource::Disk);
    app.set_preload_lines(vec![
        "queued dirs=2 previews=1".to_string(),
        "window=0..10".to_string(),
    ]);

    let screen = draw(&app, 108, 24);

    assert!(!screen.contains("Context"));
    assert!(!screen.contains("parent"));
    assert!(screen.contains("/remote/current"));
    assert!(screen.contains("photos"));
    assert!(screen.contains("Cache: disk"));
    assert!(screen.contains("queued dirs=2"));
    assert!(!screen.contains("Locations"));
    assert!(!screen.contains("favorites"));
}

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

#[test]
fn preview_panel_wraps_long_name_without_losing_suffix() {
    let mut app = TuiApp::new("/videos");
    app.set_entries(vec![file(
        1,
        "/videos/A.Knight.of.the.Seven.Kingdoms.S01E01.2160p.MAX.WEB-DL.mkv",
        5_830_i64 * 1024 * 1024,
        false,
    )]);

    let rendered = draw(&app, 108, 24);

    assert!(rendered.contains("A.Knight"));
    assert!(!rendered.contains("name=A.Knight"));
}

#[test]
fn preview_panel_uses_empty_space_for_selected_file_details() {
    let mut app = TuiApp::new("/remote/current");
    app.set_entries(vec![file(7, "/remote/current/photo.png", 8192, false)]);
    app.finish_preview_image_metadata(
        "photo.png",
        vec![
            "image=png".to_string(),
            "dimensions=320x240".to_string(),
            "terminal=metadata".to_string(),
        ],
        PreviewStatus {
            kind: PreviewKind::Image,
            fetched_bytes: 4096,
            memory_limit_bytes: 16 * 1024 * 1024,
        },
    );

    let screen = draw(&app, 108, 24);

    assert!(screen.contains("Name: photo.png"));
    assert!(screen.contains("Kind: file"));
    assert!(screen.contains("Size: 8.0 KiB"));
    assert!(screen.contains("Modified: 2023-11-14 22:13"));
    assert!(screen.contains("ID: 7"));
    assert!(screen.contains("image=png"));
    assert!(screen.contains("dimensions=320x240"));
}

#[test]
fn visual_hierarchy_uses_a_real_theme_not_default_black_and_white() {
    let mut app = TuiApp::new("/remote/current");
    app.set_entries(vec![
        file(1, "/remote/current/photos", 0, true),
        file(2, "/remote/current/readme.md", 2048, false),
    ]);
    app.set_cache_source(CacheSource::Disk);
    app.set_preload_lines(vec![
        "queued dirs=2 previews=1".to_string(),
        "window=0..10".to_string(),
    ]);

    let buffer = draw_buffer(&app, 108, 24);

    assert!(text_has_style(
        &buffer,
        "baidupcsr",
        Some(Color::Rgb(94, 234, 212)),
        Some(Color::Rgb(15, 23, 42))
    ));
    assert!(text_has_style(
        &buffer,
        "photos/",
        Some(Color::Rgb(226, 232, 240)),
        Some(Color::Rgb(14, 116, 144))
    ));
    assert!(text_has_style(
        &buffer,
        "Cache:",
        Some(Color::Rgb(148, 163, 184)),
        Some(Color::Rgb(24, 32, 52))
    ));
    assert!(text_has_style(
        &buffer,
        "1/2",
        Some(Color::Rgb(226, 232, 240)),
        Some(Color::Rgb(15, 23, 42))
    ));
    assert!(!text_has_style(
        &buffer,
        "photos/",
        Some(Color::White),
        Some(Color::Blue)
    ));
}

#[test]
fn tab_focus_only_highlights_the_active_browser_pane() {
    let mut app = TuiApp::new("/remote/current");
    app.set_entries(vec![
        file(1, "/remote/current/photos", 0, true),
        file(2, "/remote/current/readme.md", 2048, false),
    ]);

    let files_focused = draw_buffer(&app, 108, 24);
    assert!(text_has_style(
        &files_focused,
        "Files",
        Some(Color::Rgb(94, 234, 212)),
        Some(Color::Rgb(15, 23, 42))
    ));
    assert!(text_has_style(
        &files_focused,
        "Preview",
        Some(Color::Rgb(125, 211, 252)),
        Some(Color::Rgb(15, 23, 42))
    ));

    assert_eq!(app.handle_key_event(KeyEvent::Tab), None);
    let preview_focused = draw_buffer(&app, 108, 24);
    assert!(text_has_style(
        &preview_focused,
        "Files",
        Some(Color::Rgb(125, 211, 252)),
        Some(Color::Rgb(15, 23, 42))
    ));
    assert!(text_has_style(
        &preview_focused,
        "Preview",
        Some(Color::Rgb(94, 234, 212)),
        Some(Color::Rgb(15, 23, 42))
    ));
}

#[test]
fn normal_navigation_emits_runtime_event_for_preview_and_preload_refresh() {
    let mut app = TuiApp::new("/remote/current");
    app.set_entries(vec![
        file(1, "/remote/current/a.txt", 10, false),
        file(2, "/remote/current/b.txt", 10, false),
    ]);

    let event = app.handle_key_event(KeyEvent::Char('j'));

    assert_eq!(event, Some(TuiRuntimeEvent::Action(KeyAction::Down)));
    assert_eq!(app.selected_index(), Some(1));
}
