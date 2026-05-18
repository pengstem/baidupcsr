use baidupcsr_pcs::FileEntry;
use baidupcsr_tui::{AppMode, KeyEvent, TuiApp, TuiRuntimeEvent};

fn file(fs_id: i64, path: &str) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: path.to_string(),
        server_filename: path.rsplit('/').next().unwrap_or(path).to_string(),
        ctime: 1,
        mtime: 1,
        md5: String::new(),
        block_list: Vec::new(),
        size: 1,
        is_dir: false,
        has_subdir: false,
    }
}

#[test]
fn command_mode_buffers_text_and_submits_on_enter() {
    let mut app = TuiApp::new("/docs");

    assert_eq!(app.handle_key_event(KeyEvent::Char(':')), None);
    assert_eq!(app.mode(), AppMode::Command);
    for ch in "mkdir reports".chars() {
        assert_eq!(app.handle_key_event(KeyEvent::Char(ch)), None);
    }

    assert_eq!(
        app.handle_key_event(KeyEvent::Enter),
        Some(TuiRuntimeEvent::CommandSubmitted(
            "mkdir reports".to_string()
        ))
    );
    assert_eq!(app.mode(), AppMode::Normal);
    assert_eq!(app.command_input(), "");
}

#[test]
fn normal_mode_space_toggles_multi_selection_by_remote_path() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "/docs/a.txt"), file(2, "/docs/b.txt")]);

    app.handle_key_event(KeyEvent::Char(' '));
    app.handle_key_event(KeyEvent::Char('j'));
    app.handle_key_event(KeyEvent::Char(' '));

    assert_eq!(
        app.selected_paths(),
        &["/docs/a.txt".to_string(), "/docs/b.txt".to_string()]
    );
}

#[test]
fn confirm_mode_requires_explicit_yes_or_no() {
    let mut app = TuiApp::new("/docs");

    app.begin_confirm(vec![
        "delete 2 remote entries".to_string(),
        "/docs/a.txt".to_string(),
    ]);

    assert_eq!(app.mode(), AppMode::Confirm);
    assert_eq!(app.handle_key_event(KeyEvent::Char('j')), None);
    assert_eq!(
        app.handle_key_event(KeyEvent::Char('y')),
        Some(TuiRuntimeEvent::ConfirmAccepted)
    );
    assert_eq!(app.mode(), AppMode::Normal);
    assert!(app.confirm_lines().is_empty());
}

#[test]
fn search_mode_submits_current_directory_search() {
    let mut app = TuiApp::new("/docs");

    assert_eq!(app.handle_key_event(KeyEvent::Char('/')), None);
    for ch in "notes".chars() {
        assert_eq!(app.handle_key_event(KeyEvent::Char(ch)), None);
    }

    assert_eq!(
        app.handle_key_event(KeyEvent::Enter),
        Some(TuiRuntimeEvent::CommandSubmitted(
            "search notes".to_string()
        ))
    );
}
