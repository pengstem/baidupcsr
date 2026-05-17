use baidupcsr_pcs::FileEntry;

pub const fn crate_name() -> &'static str {
    "baidupcsr-tui"
}

pub const DEFAULT_KEYMAP: KeyMap = KeyMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppMode {
    Normal,
    Search,
    Command,
    Confirm,
    TaskFocus,
}

impl AppMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Search => "SEARCH",
            Self::Command => "COMMAND",
            Self::Confirm => "CONFIRM",
            Self::TaskFocus => "TASKS",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyEvent {
    Char(char),
    Esc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyAction {
    Up,
    Down,
    Back,
    Enter,
    Search,
    Command,
    Refresh,
    Quit,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyMap;

impl KeyMap {
    pub const fn action_for(self, event: KeyEvent) -> KeyAction {
        match event {
            KeyEvent::Char('j') => KeyAction::Down,
            KeyEvent::Char('k') => KeyAction::Up,
            KeyEvent::Char('h') => KeyAction::Back,
            KeyEvent::Char('l') => KeyAction::Enter,
            KeyEvent::Char('/') => KeyAction::Search,
            KeyEvent::Char(':') => KeyAction::Command,
            KeyEvent::Char('r') => KeyAction::Refresh,
            KeyEvent::Char('q') => KeyAction::Quit,
            KeyEvent::Esc => KeyAction::None,
            KeyEvent::Char(_) => KeyAction::None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiApp {
    path: String,
    entries: Vec<FileEntry>,
    selected_index: Option<usize>,
    mode: AppMode,
    background_tasks: usize,
}

impl TuiApp {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            entries: Vec::new(),
            selected_index: None,
            mode: AppMode::Normal,
            background_tasks: 0,
        }
    }

    pub fn set_entries(&mut self, entries: Vec<FileEntry>) {
        self.entries = entries;
        self.selected_index = if self.entries.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    pub fn set_background_tasks(&mut self, count: usize) {
        self.background_tasks = count;
    }

    pub fn handle_key(&mut self, event: KeyEvent) {
        if event == KeyEvent::Esc {
            self.mode = AppMode::Normal;
            return;
        }

        if self.mode != AppMode::Normal {
            return;
        }

        match DEFAULT_KEYMAP.action_for(event) {
            KeyAction::Down => self.move_down(),
            KeyAction::Up => self.move_up(),
            KeyAction::Search => self.mode = AppMode::Search,
            KeyAction::Command => self.mode = AppMode::Command,
            KeyAction::Back
            | KeyAction::Enter
            | KeyAction::Refresh
            | KeyAction::Quit
            | KeyAction::None => {}
        }
    }

    pub const fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    pub const fn mode(&self) -> AppMode {
        self.mode
    }

    pub fn status_line(&self) -> String {
        format!(
            "mode={} path={} tasks={}",
            self.mode.label(),
            self.path,
            self.background_tasks
        )
    }

    fn move_down(&mut self) {
        if let Some(index) = self.selected_index {
            let last = self.entries.len().saturating_sub(1);
            self.selected_index = Some(index.saturating_add(1).min(last));
        }
    }

    fn move_up(&mut self) {
        if let Some(index) = self.selected_index {
            self.selected_index = Some(index.saturating_sub(1));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewKind {
    Text,
    Image,
    Metadata,
    Video,
}

impl PreviewKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Metadata => "metadata",
            Self::Video => "video",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviewBudget {
    pub kind: PreviewKind,
    pub fetched_bytes: u64,
    pub memory_limit_bytes: u64,
    pub truncated: bool,
    pub disk_backed: bool,
}

impl PreviewBudget {
    pub fn status_line(&self) -> String {
        format!(
            "preview={} fetched={} limit={} truncated={} disk={}",
            self.kind.label(),
            self.fetched_bytes,
            self.memory_limit_bytes,
            self.truncated,
            self.disk_backed
        )
    }
}
