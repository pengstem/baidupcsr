use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Down,
    Up,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    Bottom,
    /// First half of `gg`.
    G,
    Parent,
    Open,
    Mark,
    Visual,
    MarkAll,
    Filter,
    Search,
    Command,
    Rename,
    Mkdir,
    Yank,
    Cut,
    Paste,
    /// First half of `dd`.
    D,
    Download,
    Sort,
    Refresh,
    Tasks,
    CancelTask,
    FocusNext,
    Help,
    Escape,
    Quit,
}

pub fn action_for(key: KeyEvent) -> Option<Action> {
    use Action as A;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    Some(match key.code {
        KeyCode::Char('d') if ctrl => A::HalfPageDown,
        KeyCode::Char('u') if ctrl => A::HalfPageUp,
        KeyCode::Char('f') if ctrl => A::PageDown,
        KeyCode::Char('b') if ctrl => A::PageUp,
        KeyCode::Char('r') if ctrl => A::Refresh,
        KeyCode::Char('c') if ctrl => A::Quit,
        _ if ctrl => return None,
        KeyCode::Char('j') | KeyCode::Down => A::Down,
        KeyCode::Char('k') | KeyCode::Up => A::Up,
        KeyCode::PageDown => A::PageDown,
        KeyCode::PageUp => A::PageUp,
        KeyCode::Char('G') | KeyCode::End => A::Bottom,
        KeyCode::Char('g') | KeyCode::Home => A::G,
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => A::Parent,
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => A::Open,
        KeyCode::Char(' ') => A::Mark,
        KeyCode::Char('v') => A::Visual,
        KeyCode::Char('V') => A::MarkAll,
        KeyCode::Char('/') => A::Filter,
        KeyCode::Char('s') => A::Search,
        KeyCode::Char(':') => A::Command,
        KeyCode::Char('r') => A::Rename,
        KeyCode::Char('a') => A::Mkdir,
        KeyCode::Char('y') => A::Yank,
        KeyCode::Char('x') => A::Cut,
        KeyCode::Char('p') => A::Paste,
        KeyCode::Char('d') => A::D,
        KeyCode::Char('D') => A::Download,
        KeyCode::Char('S') => A::Sort,
        KeyCode::Char('R') | KeyCode::F(5) => A::Refresh,
        KeyCode::Char('T') => A::Tasks,
        KeyCode::Char('c') => A::CancelTask,
        KeyCode::Tab => A::FocusNext,
        KeyCode::Char('?') => A::Help,
        KeyCode::Esc => A::Escape,
        KeyCode::Char('q') => A::Quit,
        _ => return None,
    })
}

/// The help overlay, grouped. Keep in sync with [`action_for`].
pub const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Navigate",
        &[
            ("j k ↑ ↓", "move"),
            ("h ←  ⌫", "parent directory"),
            ("l → ⏎", "open / play"),
            ("gg  G", "top / bottom"),
            ("^d ^u", "half page"),
            ("Tab", "focus preview"),
        ],
    ),
    (
        "Select",
        &[
            ("Space", "mark"),
            ("v", "visual range"),
            ("V", "mark all / none"),
            ("Esc", "clear marks, filter"),
        ],
    ),
    (
        "Files",
        &[
            ("D", "download"),
            ("y  x", "yank / cut"),
            ("p", "paste here"),
            ("dd", "delete"),
            ("r", "rename"),
            ("a", "new directory"),
        ],
    ),
    (
        "Find & view",
        &[
            ("/", "filter"),
            ("s", "search below here"),
            (":", "command"),
            ("S", "sort"),
            ("R  ^r", "refresh"),
            ("T  c", "tasks / cancel"),
            ("q", "quit"),
        ],
    ),
];
