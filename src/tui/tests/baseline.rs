use super::super::*;
use crate::pcs::FileEntry;
use ratatui::layout::Rect;

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
    assert_eq!(key_action_for(KeyEvent::Char('j')), Some(KeyAction::Down));
    assert_eq!(key_action_for(KeyEvent::Char('k')), Some(KeyAction::Up));
    assert_eq!(key_action_for(KeyEvent::Char('h')), Some(KeyAction::Back));
    assert_eq!(key_action_for(KeyEvent::Char('l')), Some(KeyAction::Enter));
    assert_eq!(key_action_for(KeyEvent::Char('q')), Some(KeyAction::Quit));
    assert_eq!(
        key_action_for(KeyEvent::Char('p')),
        Some(KeyAction::Preview)
    );
    assert_eq!(
        key_action_for(KeyEvent::Char('d')),
        Some(KeyAction::Download)
    );
    assert_eq!(key_action_for(KeyEvent::Char('x')), Some(KeyAction::Delete));
    assert_eq!(
        key_action_for(KeyEvent::Char('c')),
        Some(KeyAction::CancelTask)
    );
    assert_eq!(key_action_for(KeyEvent::Char('r')), Some(KeyAction::Rename));
    assert_eq!(
        key_action_for(KeyEvent::Char('u')),
        Some(KeyAction::Refresh)
    );
    assert_eq!(key_action_for(KeyEvent::Char(',')), Some(KeyAction::Sort));
    assert_eq!(
        key_action_for(KeyEvent::PageDown),
        Some(KeyAction::PageDown)
    );
    assert_eq!(key_action_for(KeyEvent::PageUp), Some(KeyAction::PageUp));
    assert_eq!(key_action_for(KeyEvent::Home), Some(KeyAction::Top));
    assert_eq!(key_action_for(KeyEvent::End), Some(KeyAction::Bottom));
    assert_eq!(
        key_action_for(KeyEvent::Ctrl('d')),
        Some(KeyAction::HalfPageDown)
    );
    assert_eq!(
        key_action_for(KeyEvent::Ctrl('u')),
        Some(KeyAction::HalfPageUp)
    );
}

#[test]
fn arrow_keys_mirror_vim_navigation_actions() {
    assert_eq!(key_action_for(KeyEvent::Down), Some(KeyAction::Down));
    assert_eq!(key_action_for(KeyEvent::Up), Some(KeyAction::Up));
    assert_eq!(key_action_for(KeyEvent::Left), Some(KeyAction::Back));
    assert_eq!(key_action_for(KeyEvent::Right), Some(KeyAction::Enter));
}

#[test]
fn preview_mouse_wheel_scrolls_preview_without_changing_selection() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/readme.md"), file(2, "/docs/todo.md")]);
    app.finish_preview_text(
        "readme.md",
        "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12",
        PreviewBudget {
            kind: PreviewKind::Text,
            fetched_bytes: 36,
            memory_limit_bytes: 16 * 1024 * 1024,
        },
    );

    let event = app.handle_mouse_event(MouseInput {
        kind: MouseInputKind::ScrollDown,
        column: 70,
        row: 5,
        area: Rect::new(0, 0, 96, 16),
    });

    assert_eq!(event, None);
    assert_eq!(app.selected_index(), Some(0));
    assert!(app.preview_scroll_offset > 0);
}

#[test]
fn mouse_click_on_preview_scrollbar_moves_preview_to_bottom() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/readme.md")]);
    app.finish_preview_text(
        "readme.md",
        "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16",
        PreviewBudget {
            kind: PreviewKind::Text,
            fetched_bytes: 48,
            memory_limit_bytes: 16 * 1024 * 1024,
        },
    );

    let event = app.handle_mouse_event(MouseInput {
        kind: MouseInputKind::LeftClick,
        column: 94,
        row: 10,
        area: Rect::new(0, 0, 96, 18),
    });

    assert_eq!(event, None);
    assert_eq!(app.preview_scroll_offset, 15);
}

#[test]
fn preview_text_updates_panel_content_and_budget() {
    let mut app = TuiApp::new("/docs");

    let budget = PreviewBudget {
        kind: PreviewKind::Text,
        fetched_bytes: 11,
        memory_limit_bytes: 16 * 1024 * 1024,
    };
    app.begin_preview("notes.md", "loading text preview");
    app.finish_preview_text("notes.md", "hello\nworld", budget);

    assert_eq!(app.preview_title, "notes.md");
    assert_eq!(
        app.preview_lines,
        vec!["hello".to_string(), "world".to_string()]
    );
    assert_eq!(app.preview_budget, budget);
}

#[test]
fn video_preview_status_updates_panel() {
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
        },
    );

    assert_eq!(app.preview_title, "movie.mp4");
    assert_eq!(app.preview_budget.kind, PreviewKind::Video);
    assert_eq!(app.preview_budget.memory_limit_bytes, 1024 * 1024);
    assert_eq!(app.preview_lines[0], "video=loopback");
}

#[test]
fn task_items_track_recent_speed_history() {
    let mut app = TuiApp::new("/remote");
    let mut item = TaskPanelItem {
        id: 7,
        kind: "download".to_string(),
        state: "running".to_string(),
        bytes_done: 512,
        bytes_total: Some(1024),
        speed_bytes_per_sec: 128,
        label: "movie.mkv".to_string(),
    };

    app.set_task_items(vec![item.clone()]);
    item.bytes_done = 768;
    item.speed_bytes_per_sec = 256;
    app.set_task_items(vec![item]);

    assert_eq!(
        app.task_speed_history.get(&7).map(Vec::as_slice),
        Some([128, 256].as_slice())
    );
}

#[test]
fn mode_keys_enter_filter_command_and_return_to_normal() {
    let mut app = TuiApp::new("/");

    app.handle_key_event(KeyEvent::Char('f'));
    assert_eq!(app.mode, AppMode::Filter);

    app.handle_key_event(KeyEvent::Esc);
    assert_eq!(app.mode, AppMode::Normal);

    app.handle_key_event(KeyEvent::Char('/'));
    assert_eq!(app.mode, AppMode::Filter);

    app.handle_key_event(KeyEvent::Esc);
    assert_eq!(app.mode, AppMode::Normal);

    app.handle_key_event(KeyEvent::Char('s'));
    assert_eq!(app.mode, AppMode::Search);

    app.handle_key_event(KeyEvent::Esc);
    assert_eq!(app.mode, AppMode::Normal);

    app.handle_key_event(KeyEvent::Char(':'));
    assert_eq!(app.mode, AppMode::Command);

    app.handle_key_event(KeyEvent::Esc);
    assert_eq!(app.mode, AppMode::Normal);
}

#[test]
fn slash_filters_current_entries_without_submitting_remote_search() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![
        file(1, "/notes.md"),
        file(2, "/photo.jpg"),
        file(3, "/todo.txt"),
    ]);

    assert_eq!(app.handle_key_event(KeyEvent::Char('/')), None);
    for ch in "note".chars() {
        assert_eq!(app.handle_key_event(KeyEvent::Char(ch)), None);
    }

    assert_eq!(app.mode, AppMode::Filter);
    assert_eq!(app.filter_input, "note");
    assert_eq!(app.filtered_entry_count(), 1);
    assert_eq!(
        app.selected_entry().map(|entry| entry.path.as_str()),
        Some("/notes.md")
    );
    assert_eq!(app.handle_key_event(KeyEvent::Enter), None);
    assert_eq!(app.mode, AppMode::Normal);
    assert_eq!(app.filter_input, "note");

    assert_eq!(app.handle_key_event(KeyEvent::Char('/')), None);
    for ch in "zzz".chars() {
        assert_eq!(app.handle_key_event(KeyEvent::Char(ch)), None);
    }
    assert_eq!(app.filtered_entry_count(), 0);
    assert_eq!(app.handle_key_event(KeyEvent::Esc), None);
    assert_eq!(app.filter_input, "");
    assert_eq!(app.filtered_entry_count(), 3);
}

#[test]
fn f_filters_current_entries_without_submitting_remote_search() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![
        file(1, "/notes.md"),
        file(2, "/photo.jpg"),
        file(3, "/todo.txt"),
    ]);

    assert_eq!(app.handle_key_event(KeyEvent::Char('f')), None);
    for ch in "photo".chars() {
        assert_eq!(app.handle_key_event(KeyEvent::Char(ch)), None);
    }

    assert_eq!(app.mode, AppMode::Filter);
    assert_eq!(app.filtered_entry_count(), 1);
    assert_eq!(
        app.selected_entry().map(|entry| entry.path.as_str()),
        Some("/photo.jpg")
    );
}

#[test]
fn s_submits_remote_search_input() {
    let mut app = TuiApp::new("/docs");

    assert_eq!(app.handle_key_event(KeyEvent::Char('s')), None);
    for ch in "notes".chars() {
        assert_eq!(app.handle_key_event(KeyEvent::Char(ch)), None);
    }

    assert_eq!(
        app.handle_key_event(KeyEvent::Enter),
        Some(TuiRuntimeEvent::SearchSubmitted("notes".to_string()))
    );
    assert_eq!(app.mode, AppMode::Normal);
    assert_eq!(app.search_input, "");
}

#[test]
fn r_prefills_selected_name_and_submits_rename_input() {
    let mut app = TuiApp::new("/docs");
    let mut entry = file(1, "/docs/notes.md");
    entry.server_filename = "notes.md".to_string();
    app.set_entries(vec![entry]);

    assert_eq!(app.handle_key_event(KeyEvent::Char('r')), None);
    assert_eq!(app.mode, AppMode::Rename);
    assert_eq!(app.rename_input, "notes.md");
    for _ in 0.."notes.md".len() {
        app.handle_key_event(KeyEvent::Backspace);
    }
    for ch in "renamed.md".chars() {
        app.handle_key_event(KeyEvent::Char(ch));
    }

    assert_eq!(
        app.handle_key_event(KeyEvent::Enter),
        Some(TuiRuntimeEvent::RenameSubmitted("renamed.md".to_string()))
    );
    assert_eq!(app.mode, AppMode::Normal);
    assert!(app.rename_input.is_empty());
}

#[test]
fn comma_sort_menu_changes_order_and_toggles_direction() {
    let mut older = file(1, "/docs/older.txt");
    older.server_filename = "older.txt".to_string();
    older.mtime = 10;
    let mut newer = file(2, "/docs/newer.txt");
    newer.server_filename = "newer.txt".to_string();
    newer.mtime = 20;

    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![older, newer]);

    assert_eq!(app.handle_key_event(KeyEvent::Char(',')), None);
    assert_eq!(app.mode, AppMode::Sort);
    assert_eq!(app.handle_key_event(KeyEvent::Char('m')), None);
    assert_eq!(app.mode, AppMode::Normal);
    assert_eq!(app.sort_mode.key, SortKey::Modified);
    assert_eq!(app.sort_mode.direction, SortDirection::Descending);
    assert_eq!(
        app.entries()
            .iter()
            .map(|entry| entry.server_filename.as_str())
            .collect::<Vec<_>>(),
        vec!["newer.txt", "older.txt"]
    );

    app.handle_key_event(KeyEvent::Char(','));
    app.handle_key_event(KeyEvent::Char('m'));
    assert_eq!(app.sort_mode.direction, SortDirection::Ascending);
    assert_eq!(
        app.entries()
            .iter()
            .map(|entry| entry.server_filename.as_str())
            .collect::<Vec<_>>(),
        vec!["older.txt", "newer.txt"]
    );
}

#[test]
fn type_sort_groups_by_extension_after_directories() {
    let mut archive = file(1, "/docs/archive.zip");
    archive.server_filename = "archive.zip".to_string();
    let mut image = file(2, "/docs/photo.jpg");
    image.server_filename = "photo.jpg".to_string();
    let mut text = file(3, "/docs/readme.txt");
    text.server_filename = "readme.txt".to_string();
    let mut dir = directory(4, "/docs/folder");
    dir.server_filename = "folder".to_string();

    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![text, image, archive, dir]);
    app.handle_key_event(KeyEvent::Char(','));
    app.handle_key_event(KeyEvent::Char('t'));

    assert_eq!(
        app.entries()
            .iter()
            .map(|entry| entry.server_filename.as_str())
            .collect::<Vec<_>>(),
        vec!["folder", "photo.jpg", "readme.txt", "archive.zip"]
    );
}

#[test]
fn search_results_restore_original_directory_on_exit() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/readme.md"), file(2, "/docs/todo.md")]);
    app.handle_key_event(KeyEvent::Char('j'));

    app.begin_search_results("/docs", "photo");
    app.append_search_results(vec![file(3, "/docs/nested/photo.jpg")], 2, 8);

    assert!(app.is_search_results());
    assert_eq!(app.entries().len(), 1);
    assert_eq!(app.exit_search_results(), Some("/docs".to_string()));
    assert!(!app.is_search_results());
    assert_eq!(app.entries().len(), 2);
    assert_eq!(
        app.selected_entry().map(|entry| entry.path.as_str()),
        Some("/docs/todo.md")
    );
}

#[test]
fn quit_with_background_tasks_requires_confirmation() {
    let mut app = TuiApp::new("/docs");
    app.set_background_tasks(2);

    assert_eq!(app.handle_key_event(KeyEvent::Char('q')), None);
    assert_eq!(app.mode, AppMode::Confirm);
    assert_eq!(
        app.confirm_lines,
        vec!["Quit with 2 tasks still running?".to_string()]
    );

    assert_eq!(app.handle_key_event(KeyEvent::Char('n')), None);
    assert_eq!(app.mode, AppMode::Normal);

    assert_eq!(app.handle_key_event(KeyEvent::Char('q')), None);
    assert_eq!(
        app.handle_key_event(KeyEvent::Char('y')),
        Some(TuiRuntimeEvent::Action(KeyAction::Quit))
    );
}

#[test]
fn command_mode_buffers_text_and_submits_on_enter() {
    let mut app = TuiApp::new("/docs");

    assert_eq!(app.handle_key_event(KeyEvent::Char(':')), None);
    assert_eq!(app.mode, AppMode::Command);
    for ch in "mkdir reports".chars() {
        assert_eq!(app.handle_key_event(KeyEvent::Char(ch)), None);
    }

    assert_eq!(
        app.handle_key_event(KeyEvent::Enter),
        Some(TuiRuntimeEvent::CommandSubmitted(
            "mkdir reports".to_string()
        ))
    );
    assert_eq!(app.mode, AppMode::Normal);
    assert_eq!(app.command_input, "");
}

#[test]
fn confirm_mode_requires_explicit_yes_or_no() {
    let mut app = TuiApp::new("/docs");

    app.begin_confirm(vec![
        "delete 2 remote entries".to_string(),
        "/docs/a.txt".to_string(),
    ]);

    assert_eq!(app.mode, AppMode::Confirm);
    assert_eq!(app.handle_key_event(KeyEvent::Char('j')), None);
    assert_eq!(
        app.handle_key_event(KeyEvent::Char('y')),
        Some(TuiRuntimeEvent::ConfirmAccepted)
    );
    assert_eq!(app.mode, AppMode::Normal);
    assert!(app.confirm_lines.is_empty());
}

#[test]
fn mouse_wheel_scrolls_view_and_keeps_selection_visible() {
    let mut app = TuiApp::new("/docs");
    let entries: Vec<FileEntry> = (0..20)
        .map(|index| file(index, &format!("/docs/item-{index:02}.txt")))
        .collect();
    app.set_entries(entries);

    assert_eq!(
        app.handle_mouse_event(MouseInput {
            kind: MouseInputKind::ScrollDown,
            column: 24,
            row: 5,
            area: Rect::new(0, 0, 96, 24),
        }),
        Some(TuiRuntimeEvent::Action(KeyAction::Select))
    );
    assert_eq!(app.selected_index(), Some(3));
    assert_eq!(app.visible_entry_range(4), 2..6);
}

#[test]
fn mouse_click_on_files_scrollbar_moves_view_to_bottom() {
    let mut app = TuiApp::new("/docs");
    let entries: Vec<FileEntry> = (0..40)
        .map(|index| file(index, &format!("/docs/item-{index:02}.txt")))
        .collect();
    app.set_entries(entries);

    let event = app.handle_mouse_event(MouseInput {
        kind: MouseInputKind::LeftClick,
        column: 44,
        row: 16,
        area: Rect::new(0, 0, 96, 24),
    });

    assert_eq!(event, Some(TuiRuntimeEvent::Action(KeyAction::Select)));
    assert_eq!(app.visible_entry_range(13), 27..40);
    assert_eq!(app.selected_index(), Some(27));
}

#[test]
fn mouse_drag_on_files_scrollbar_keeps_controlling_files_after_leaving_column() {
    let mut app = TuiApp::new("/docs");
    let entries: Vec<FileEntry> = (0..40)
        .map(|index| file(index, &format!("/docs/item-{index:02}.txt")))
        .collect();
    app.set_entries(entries);

    let area = Rect::new(0, 0, 96, 24);
    let _ = app.handle_mouse_event(MouseInput {
        kind: MouseInputKind::LeftClick,
        column: 44,
        row: 4,
        area,
    });

    let event = app.handle_mouse_event(MouseInput {
        kind: MouseInputKind::LeftDrag,
        column: 40,
        row: 16,
        area,
    });

    assert_eq!(event, Some(TuiRuntimeEvent::Action(KeyAction::Select)));
    assert_eq!(app.visible_entry_range(13), 27..40);
}

#[test]
fn directory_load_state_replaces_path_entries_and_status() {
    let mut app = TuiApp::new("/");
    app.set_entries(vec![directory(1, "/docs")]);

    app.begin_directory_refresh("/docs", Vec::new(), None, None);
    assert_eq!(app.path(), "/docs");
    assert_eq!(app.entries(), &[]);
    assert_eq!(app.selected_index(), None);
    assert_eq!(app.notice.as_deref(), Some("loading /docs"));
    assert_eq!(app.request_state, DirectoryRequestState::Loading);

    app.finish_directory_success(
        "/docs",
        vec![file(2, "/docs/readme.md"), directory(3, "/docs/src")],
        Some(0),
        None,
    );

    assert_eq!(app.path(), "/docs");
    assert_eq!(app.entries().len(), 2);
    assert_eq!(app.selected_index(), Some(0));
    assert_eq!(app.notice.as_deref(), Some("loaded /docs"));
    assert_eq!(app.request_state, DirectoryRequestState::Idle);
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
    assert_eq!(app.notice.as_deref(), Some("remote_list_error: 403"));
    assert_eq!(app.request_state, DirectoryRequestState::Failed);
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
    assert_eq!(app.request_state, DirectoryRequestState::Refreshing);
    assert_eq!(app.entries().len(), 2);
    assert_eq!(
        app.selected_entry().map(|entry| entry.path.as_str()),
        Some("/docs/src")
    );
    assert_eq!(app.notice.as_deref(), Some("refreshing /docs from cache"));
}
