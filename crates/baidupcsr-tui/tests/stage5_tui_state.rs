use baidupcsr_pcs::FileEntry;
use baidupcsr_tui::{
    AppMode, KeyAction, KeyEvent, PreviewBudget, PreviewKind, TuiApp, DEFAULT_KEYMAP,
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

    assert_eq!(app.status_line(), "mode=NORMAL path=/docs tasks=2");
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
