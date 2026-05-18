use baidupcsr_pcs::FileEntry;
use baidupcsr_tui::{
    AppMode, DirectoryRequestState, KeyAction, KeyEvent, PreviewBudget, PreviewKind, TuiApp,
    TuiRuntimeEvent, DEFAULT_KEYMAP,
};

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
fn default_keymap_exposes_vim_navigation() {
    assert_eq!(
        DEFAULT_KEYMAP.action_for(KeyEvent::Char('j')),
        KeyAction::Down
    );
    assert_eq!(
        DEFAULT_KEYMAP.action_for(KeyEvent::Char('k')),
        KeyAction::Up
    );
    assert_eq!(
        DEFAULT_KEYMAP.action_for(KeyEvent::Char('h')),
        KeyAction::Back
    );
    assert_eq!(
        DEFAULT_KEYMAP.action_for(KeyEvent::Char('l')),
        KeyAction::Enter
    );
    assert_eq!(
        DEFAULT_KEYMAP.action_for(KeyEvent::Char('q')),
        KeyAction::Quit
    );
    assert_eq!(
        DEFAULT_KEYMAP.action_for(KeyEvent::Char('p')),
        KeyAction::Preview
    );
    assert_eq!(
        DEFAULT_KEYMAP.action_for(KeyEvent::Char('d')),
        KeyAction::Download
    );
    assert_eq!(
        DEFAULT_KEYMAP.action_for(KeyEvent::Char('x')),
        KeyAction::CancelTask
    );
}

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

#[test]
fn normal_mode_navigation_keeps_selection_in_bounds() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/a.txt"), file(2, "/docs/b.txt")]);

    app.handle_key(KeyEvent::Char('j'));
    app.handle_key(KeyEvent::Char('j'));
    assert_eq!(app.selected_index(), Some(1));

    app.handle_key(KeyEvent::Char('k'));
    app.handle_key(KeyEvent::Char('k'));
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
fn mouse_wheel_moves_selection_like_keyboard_navigation() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/a.txt"), file(2, "/docs/b.txt")]);

    assert_eq!(
        app.handle_mouse_event(baidupcsr_tui::MouseInput {
            kind: baidupcsr_tui::MouseInputKind::ScrollDown,
            column: 24,
            row: 5,
            area: ratatui::layout::Rect::new(0, 0, 96, 24),
        }),
        Some(TuiRuntimeEvent::Action(KeyAction::Down))
    );
    assert_eq!(app.selected_index(), Some(1));
}

#[test]
fn mode_keys_enter_search_command_and_return_to_normal() {
    let mut app = TuiApp::new("/");

    app.handle_key(KeyEvent::Char('/'));
    assert_eq!(app.mode(), AppMode::Search);

    app.handle_key(KeyEvent::Esc);
    assert_eq!(app.mode(), AppMode::Normal);

    app.handle_key(KeyEvent::Char(':'));
    assert_eq!(app.mode(), AppMode::Command);

    app.handle_key(KeyEvent::Esc);
    assert_eq!(app.mode(), AppMode::Normal);
}

#[test]
fn status_line_reports_mode_path_and_task_count() {
    let mut app = TuiApp::new("/docs");
    app.set_background_tasks(2);

    assert_eq!(
        app.status_line(),
        "mode=NORMAL path=/docs request=idle cache_age=none tasks=2"
    );
}

#[test]
fn preview_budget_line_reports_memory_and_disk_boundary() {
    let budget = PreviewBudget {
        kind: PreviewKind::Text,
        fetched_bytes: 1024,
        memory_limit_bytes: 16 * 1024 * 1024,
        truncated: false,
        disk_backed: false,
    };

    assert_eq!(
        budget.status_line(),
        "preview=text fetched=1024 limit=16777216 truncated=false disk=false"
    );
}

#[test]
fn preview_text_updates_panel_content_and_budget() {
    let mut app = TuiApp::new("/docs");

    app.begin_preview("notes.md", "loading text preview");
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

    assert_eq!(app.preview_title(), "notes.md");
    assert_eq!(
        app.preview_lines(),
        &["hello".to_string(), "world".to_string()]
    );
    assert_eq!(
        app.preview_budget().status_line(),
        "preview=text fetched=11 limit=16777216 truncated=false disk=false"
    );
}

#[test]
fn video_preview_status_updates_panel_without_disk_cache() {
    let mut app = TuiApp::new("/videos");

    app.finish_preview_video(
        "movie.mp4",
        vec![
            "video=loopback".to_string(),
            "listen=127.0.0.1:39177".to_string(),
            "player=mpv".to_string(),
            "cache=0/1048576".to_string(),
        ],
        PreviewBudget {
            kind: PreviewKind::Video,
            fetched_bytes: 0,
            memory_limit_bytes: 1024 * 1024,
            truncated: false,
            disk_backed: false,
        },
    );

    assert_eq!(app.preview_title(), "movie.mp4");
    assert_eq!(app.preview_budget().kind, PreviewKind::Video);
    assert_eq!(app.preview_budget().memory_limit_bytes, 1024 * 1024);
    assert!(!app.preview_budget().disk_backed);
    assert_eq!(app.preview_lines()[0], "video=loopback");
}

#[test]
fn task_lines_update_bottom_panel_state() {
    let mut app = TuiApp::new("/remote");

    app.set_task_lines(vec![
        "#1 download running 512/1024".to_string(),
        "#2 download failed network".to_string(),
    ]);

    assert_eq!(
        app.task_lines(),
        &[
            "#1 download running 512/1024".to_string(),
            "#2 download failed network".to_string(),
        ]
    );
}

#[test]
fn selected_directory_path_only_returns_directories() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![directory(1, "/docs"), file(2, "/notes.txt")]);

    assert_eq!(app.selected_directory_path(), Some("/docs".to_string()));

    app.handle_key(KeyEvent::Char('j'));
    assert_eq!(app.selected_directory_path(), None);
}

#[test]
fn directory_load_state_replaces_path_entries_and_status() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![directory(1, "/docs")]);

    app.begin_directory_refresh("/docs", Vec::new(), None, None);
    assert_eq!(app.path(), "/docs");
    assert_eq!(app.entries(), &[]);
    assert_eq!(app.selected_index(), None);
    assert_eq!(app.notice(), Some("loading /docs"));
    assert_eq!(app.request_state(), DirectoryRequestState::Loading);
    assert_eq!(
        app.status_line(),
        "mode=NORMAL path=/docs request=loading cache_age=none tasks=1"
    );

    app.finish_directory_success(
        "/docs",
        vec![file(2, "/docs/readme.md"), directory(3, "/docs/src")],
        Some(0),
        None,
    );

    assert_eq!(app.path(), "/docs");
    assert_eq!(app.entries().len(), 2);
    assert_eq!(app.selected_index(), Some(0));
    assert_eq!(app.notice(), Some("loaded /docs"));
    assert_eq!(
        app.status_line(),
        "mode=NORMAL path=/docs request=idle cache_age=0s tasks=0"
    );
}

#[test]
fn directory_load_failure_keeps_cached_entries_and_reports_error() {
    let mut app = TuiApp::new("/docs");

    app.begin_directory_refresh(
        "/missing",
        vec![file(1, "/missing/cached.txt")],
        Some(7),
        None,
    );
    app.finish_directory_error(
        "/missing",
        vec![file(1, "/missing/cached.txt")],
        Some(7),
        "remote_list_error: 403",
        None,
    );

    assert_eq!(app.path(), "/missing");
    assert_eq!(app.entries().len(), 1);
    assert_eq!(app.notice(), Some("remote_list_error: 403"));
    assert_eq!(
        app.status_line(),
        "mode=NORMAL path=/missing request=failed cache_age=7s tasks=0"
    );
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
fn cached_directory_refresh_keeps_stale_entries_visible() {
    let mut app = TuiApp::new("/");

    app.begin_directory_refresh(
        "/docs",
        vec![file(1, "/docs/cached.txt"), directory(2, "/docs/src")],
        Some(12),
        Some("/docs/src"),
    );

    assert_eq!(app.path(), "/docs");
    assert_eq!(app.request_state(), DirectoryRequestState::Refreshing);
    assert_eq!(app.entries().len(), 2);
    assert_eq!(
        app.selected_entry().map(|entry| entry.path.as_str()),
        Some("/docs/src")
    );
    assert_eq!(app.notice(), Some("refreshing /docs from cache"));
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
