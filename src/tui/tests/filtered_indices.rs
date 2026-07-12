use super::super::*;
use crate::pcs::FileEntry;

fn file(fs_id: i64, name: &str, mtime: i64) -> FileEntry {
    FileEntry {
        fs_id,
        app_id: 266719,
        path: format!("/docs/{name}"),
        server_filename: name.to_string(),
        ctime: 1,
        mtime,
        md5: String::new(),
        block_list: Vec::new(),
        size: 1,
        is_dir: false,
        has_subdir: false,
    }
}

fn enter_filter(app: &mut TuiApp, value: &str) {
    app.handle_key_event(KeyEvent::Char('f'));
    for ch in value.chars() {
        app.handle_key_event(KeyEvent::Char(ch));
    }
    app.handle_key_event(KeyEvent::Enter);
}

#[test]
fn filtered_indices_reuses_cached_results_when_inputs_are_unchanged() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![
        file(1, "alpha.txt", 1),
        file(2, "alpha.md", 2),
        file(3, "beta.txt", 3),
    ]);
    enter_filter(&mut app, "alpha");

    let first = app.filtered_indices();
    let second = app.filtered_indices();

    assert_eq!(first, second);
    assert_eq!(first.as_ptr(), second.as_ptr());
    assert_eq!(first.len(), 2);
}

#[test]
fn filtered_indices_refreshes_when_entries_change() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "alpha.txt", 1), file(2, "beta.txt", 2)]);
    enter_filter(&mut app, "alpha");
    assert_eq!(app.filtered_indices(), &[0]);

    app.set_entries(vec![file(3, "gamma.txt", 3), file(4, "delta.txt", 4)]);

    assert!(app.filtered_indices().is_empty());
}

#[test]
fn filtered_indices_refreshes_when_filter_changes() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "alpha.txt", 1), file(2, "beta.txt", 2)]);

    enter_filter(&mut app, "alpha");
    assert_eq!(app.filtered_indices(), &[0]);

    enter_filter(&mut app, "beta");

    assert_eq!(app.filtered_indices(), &[1]);
}

#[test]
fn filtered_indices_refreshes_when_sort_changes() {
    let mut app = TuiApp::new("/docs");
    app.set_entries(vec![file(1, "alpha.txt", 1), file(2, "beta.txt", 2)]);
    enter_filter(&mut app, "alpha");
    assert_eq!(app.filtered_indices(), &[0]);

    app.choose_sort_key(SortKey::Modified);

    assert_eq!(app.entries()[1].server_filename, "alpha.txt");
    assert_eq!(app.filtered_indices(), &[1]);
}
