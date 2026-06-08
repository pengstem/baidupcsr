use baidupcsr_pcs::FileEntry;
use baidupcsr_tui::{KeyAction, KeyEvent, TuiApp, TuiRuntimeEvent};

fn file(fs_id: i64, path: &str) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: path.to_string(),
        server_filename: path.trim_start_matches('/').to_string(),
        ctime: 1,
        mtime: 1,
        md5: String::new(),
        block_list: Vec::new(),
        size: 1,
        is_dir: false,
        has_subdir: false,
    }
}

fn directory(fs_id: i64, path: &str) -> FileEntry {
    FileEntry {
        is_dir: true,
        has_subdir: true,
        size: 0,
        ..file(fs_id, path)
    }
}

#[test]
fn normal_mode_navigation_keeps_selection_in_bounds() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/a.txt"), file(2, "/docs/b.txt")]);

    app.handle_key_event(KeyEvent::Char('j'));
    app.handle_key_event(KeyEvent::Char('j'));
    assert_eq!(app.selected_index(), Some(1));

    app.handle_key_event(KeyEvent::Char('k'));
    app.handle_key_event(KeyEvent::Char('k'));
    assert_eq!(app.selected_index(), Some(0));
}

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
        column: 24,
        row: 5,
        area: ratatui::layout::Rect::new(0, 0, 96, 24),
    });

    assert_eq!(app.selected_index(), Some(1));
    assert_eq!(event, Some(TuiRuntimeEvent::Action(KeyAction::Select)));
}

#[test]
fn mouse_wheel_moves_selection_even_when_all_entries_are_visible() {
    let mut app = TuiApp::new("/docs");
    let entries: Vec<FileEntry> = (0..5)
        .map(|index| file(index, &format!("/docs/item-{index:02}.txt")))
        .collect();
    app.set_entries(entries);

    let event = app.handle_mouse_event(baidupcsr_tui::MouseInput {
        kind: baidupcsr_tui::MouseInputKind::ScrollDown,
        column: 24,
        row: 5,
        area: ratatui::layout::Rect::new(0, 0, 96, 24),
    });

    assert_eq!(event, Some(TuiRuntimeEvent::Action(KeyAction::Select)));
    assert_eq!(app.selected_index(), Some(3));
}

#[test]
fn mouse_double_click_enters_directories_and_previews_files() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![
        directory(1, "/docs/videos"),
        file(2, "/docs/readme.md"),
    ]);

    let directory_click = baidupcsr_tui::MouseInput {
        kind: baidupcsr_tui::MouseInputKind::LeftClick,
        column: 24,
        row: 4,
        area: ratatui::layout::Rect::new(0, 0, 96, 24),
    };
    assert_eq!(
        app.handle_mouse_event(directory_click),
        Some(TuiRuntimeEvent::Action(KeyAction::Select))
    );
    assert_eq!(
        app.handle_mouse_event(directory_click),
        Some(TuiRuntimeEvent::Action(KeyAction::Enter))
    );

    let file_click = baidupcsr_tui::MouseInput {
        row: 5,
        ..directory_click
    };
    assert_eq!(
        app.handle_mouse_event(file_click),
        Some(TuiRuntimeEvent::Action(KeyAction::Select))
    );
    assert_eq!(
        app.handle_mouse_event(file_click),
        Some(TuiRuntimeEvent::Action(KeyAction::Preview))
    );
}

#[test]
fn mouse_second_click_after_timeout_stays_a_selection() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/readme.md")]);
    let click = baidupcsr_tui::MouseInput {
        kind: baidupcsr_tui::MouseInputKind::LeftClick,
        column: 24,
        row: 4,
        area: ratatui::layout::Rect::new(0, 0, 96, 24),
    };

    assert_eq!(
        app.handle_mouse_event(click),
        Some(TuiRuntimeEvent::Action(KeyAction::Select))
    );
    for _ in 0..6 {
        app.advance_tick();
    }

    assert_eq!(
        app.handle_mouse_event(click),
        Some(TuiRuntimeEvent::Action(KeyAction::Select))
    );
}

#[test]
fn mouse_click_on_header_breadcrumb_requests_that_path() {
    let mut app = TuiApp::new("/docs/videos/2026");

    let event = app.handle_mouse_event(baidupcsr_tui::MouseInput {
        kind: baidupcsr_tui::MouseInputKind::LeftClick,
        column: 20,
        row: 1,
        area: ratatui::layout::Rect::new(0, 0, 96, 24),
    });

    assert_eq!(
        event,
        Some(TuiRuntimeEvent::PathRequested("/docs".to_string()))
    );
}

#[test]
fn normal_mode_supports_large_directory_navigation_keys() {
    let mut app = TuiApp::new("/");
    let entries: Vec<FileEntry> = (0..30)
        .map(|index| file(index, &format!("/item-{index:02}.txt")))
        .collect();
    app.set_entries(entries);

    assert_eq!(
        app.handle_key_event(KeyEvent::PageDown),
        Some(TuiRuntimeEvent::Action(KeyAction::PageDown))
    );
    assert_eq!(app.selected_index(), Some(10));

    assert_eq!(
        app.handle_key_event(KeyEvent::Ctrl('u')),
        Some(TuiRuntimeEvent::Action(KeyAction::HalfPageUp))
    );
    assert_eq!(app.selected_index(), Some(5));

    assert_eq!(
        app.handle_key_event(KeyEvent::End),
        Some(TuiRuntimeEvent::Action(KeyAction::Bottom))
    );
    assert_eq!(app.selected_index(), Some(29));

    assert_eq!(
        app.handle_key_event(KeyEvent::Home),
        Some(TuiRuntimeEvent::Action(KeyAction::Top))
    );
    assert_eq!(app.selected_index(), Some(0));

    assert_eq!(
        app.handle_key_event(KeyEvent::Char('G')),
        Some(TuiRuntimeEvent::Action(KeyAction::Bottom))
    );
    assert_eq!(app.selected_index(), Some(29));

    assert_eq!(app.handle_key_event(KeyEvent::Char('g')), None);
    assert_eq!(
        app.handle_key_event(KeyEvent::Char('g')),
        Some(TuiRuntimeEvent::Action(KeyAction::Top))
    );
    assert_eq!(app.selected_index(), Some(0));
}

#[test]
fn parent_path_handles_root_and_nested_paths() {
    assert_eq!(TuiApp::parent_path("/"), None);
    assert_eq!(TuiApp::parent_path("/docs"), Some("/".to_string()));
    assert_eq!(
        TuiApp::parent_path("/docs/books/"),
        Some("/docs".to_string())
    );
}

#[test]
fn entries_are_sorted_directory_first_by_name() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![
        file(1, "/zeta.txt"),
        directory(2, "/beta"),
        file(3, "/alpha.txt"),
        directory(4, "/alpha"),
    ]);

    let names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.server_filename.as_str())
        .collect();

    assert_eq!(names, vec!["alpha", "beta", "alpha.txt", "zeta.txt"]);
}
