use baidupcsr_pcs::FileEntry;
use ratatui::{
    crossterm::event::{self, Event as CrosstermEvent, KeyCode},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::{io, time::Duration};

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
    notice: Option<String>,
}

impl TuiApp {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            entries: Vec::new(),
            selected_index: None,
            mode: AppMode::Normal,
            background_tasks: 0,
            notice: None,
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

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
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

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_index
            .and_then(|index| self.entries.get(index))
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
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

pub fn render_app(frame: &mut Frame<'_>, app: &TuiApp, preview: Option<PreviewBudget>) {
    let [header, body, tasks, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(frame.area());

    let [locations, files, preview_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(22),
            Constraint::Percentage(45),
            Constraint::Min(28),
        ])
        .areas(body);

    frame.render_widget(header_widget(app), header);
    frame.render_widget(locations_widget(app), locations);
    frame.render_widget(files_widget(app), files);
    frame.render_widget(preview_widget(app, preview), preview_area);
    frame.render_widget(tasks_widget(app), tasks);
    frame.render_widget(status_widget(app), status);
}

pub fn run_terminal_app(app: TuiApp) -> io::Result<()> {
    let mut app = app;
    let mut terminal = ratatui::init();
    let result = run_terminal_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run_terminal_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut TuiApp) -> io::Result<()> {
    loop {
        terminal.draw(|frame| {
            render_app(
                frame,
                app,
                Some(PreviewBudget {
                    kind: PreviewKind::Metadata,
                    fetched_bytes: 0,
                    memory_limit_bytes: 16 * 1024 * 1024,
                    truncated: false,
                    disk_backed: false,
                }),
            );
        })?;

        if event::poll(Duration::from_millis(100))? {
            let CrosstermEvent::Key(key) = event::read()? else {
                continue;
            };
            let Some(tui_key) = map_crossterm_key(key.code) else {
                continue;
            };
            if DEFAULT_KEYMAP.action_for(tui_key) == KeyAction::Quit
                && app.mode() == AppMode::Normal
            {
                break;
            }
            app.handle_key(tui_key);
        }
    }

    Ok(())
}

fn map_crossterm_key(code: KeyCode) -> Option<KeyEvent> {
    match code {
        KeyCode::Char(value) => Some(KeyEvent::Char(value)),
        KeyCode::Esc => Some(KeyEvent::Esc),
        _ => None,
    }
}

fn header_widget(app: &TuiApp) -> Paragraph<'_> {
    Paragraph::new(Line::from(vec![
        Span::styled(
            "baidupcsr",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(app.path()),
    ]))
    .block(Block::bordered().title("Session"))
}

fn locations_widget(app: &TuiApp) -> Paragraph<'_> {
    Paragraph::new(vec![
        Line::raw("workdir"),
        Line::raw(app.path()),
        Line::raw(""),
        Line::raw("recent"),
        Line::raw("/"),
        Line::raw(""),
        Line::raw("favorites"),
        Line::raw(app.path()),
    ])
    .block(Block::bordered().title("Locations"))
    .wrap(Wrap { trim: true })
}

fn files_widget(app: &TuiApp) -> List<'_> {
    let items: Vec<ListItem<'_>> = app
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let marker = if app.selected_index() == Some(index) {
                ">"
            } else {
                " "
            };
            let suffix = if entry.is_dir { "/" } else { "" };
            let size = if entry.is_dir {
                "-".to_string()
            } else {
                entry.size.to_string()
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::new().fg(Color::Yellow)),
                Span::raw(" "),
                Span::raw(entry.server_filename.as_str()),
                Span::raw(suffix),
                Span::raw("  "),
                Span::styled(size, Style::new().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    List::new(items).block(Block::bordered().title("Files"))
}

fn preview_widget(app: &TuiApp, preview: Option<PreviewBudget>) -> Paragraph<'_> {
    let selected = app.selected_entry();
    let title = selected
        .map(|entry| entry.server_filename.as_str())
        .or_else(|| app.notice())
        .unwrap_or("metadata");
    let kind = selected
        .map(|entry| if entry.is_dir { "directory" } else { "file" })
        .unwrap_or("none");
    let budget = preview.unwrap_or(PreviewBudget {
        kind: PreviewKind::Metadata,
        fetched_bytes: 0,
        memory_limit_bytes: 0,
        truncated: false,
        disk_backed: false,
    });

    let lines = vec![
        Line::raw(title.to_string()),
        Line::raw(kind.to_string()),
        Line::raw(""),
        Line::raw(format!("preview={}", budget.kind.label())),
        Line::raw(format!("fetched={}", budget.fetched_bytes)),
        Line::raw(format!("limit={}", budget.memory_limit_bytes)),
        Line::raw(format!("truncated={}", budget.truncated)),
        Line::raw(format!("disk={}", budget.disk_backed)),
    ];

    Paragraph::new(lines)
        .block(Block::bordered().title("Preview"))
        .wrap(Wrap { trim: true })
}

fn tasks_widget(app: &TuiApp) -> Paragraph<'_> {
    let mut lines = vec![Line::raw(format!(
        "refresh=idle preview=idle background={}",
        app.background_tasks
    ))];
    if let Some(notice) = app.notice() {
        lines.push(Line::raw(notice.to_string()));
    }
    Paragraph::new(lines).block(Block::bordered().title("Tasks"))
}

fn status_widget(app: &TuiApp) -> Paragraph<'_> {
    Paragraph::new(Line::raw(format!(
        "{}  keys j/k h/l / : r q",
        app.status_line()
    )))
    .style(Style::new().fg(Color::Black).bg(Color::Gray))
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
