use baidupcsr_pcs::FileEntry;
use ratatui::{
    crossterm::event::{self, Event as CrosstermEvent, KeyCode},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::{cmp::Ordering, io, ops::Range, time::Duration};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    Enter,
    Backspace,
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
    Preview,
    Download,
    CancelTask,
    ToggleSelect,
    Quit,
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TuiRuntimeEvent {
    Tick,
    Action(KeyAction),
    CommandSubmitted(String),
    ConfirmAccepted,
    ConfirmRejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryRequestState {
    Idle,
    Loading,
    Refreshing,
    Failed,
}

impl DirectoryRequestState {
    const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Refreshing => "refreshing",
            Self::Failed => "failed",
        }
    }
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
            KeyEvent::Char('p') => KeyAction::Preview,
            KeyEvent::Char('d') => KeyAction::Download,
            KeyEvent::Char('x') => KeyAction::CancelTask,
            KeyEvent::Char(' ') => KeyAction::ToggleSelect,
            KeyEvent::Char('q') => KeyAction::Quit,
            KeyEvent::Esc => KeyAction::None,
            KeyEvent::Enter | KeyEvent::Backspace => KeyAction::None,
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
    request_state: DirectoryRequestState,
    cache_age_seconds: Option<u64>,
    preview_title: String,
    preview_lines: Vec<String>,
    preview_budget: PreviewBudget,
    task_lines: Vec<String>,
    command_input: String,
    selected_paths: Vec<String>,
    confirm_lines: Vec<String>,
}

impl TuiApp {
    pub fn new(path: &str) -> Self {
        Self {
            path: normalize_path(path),
            entries: Vec::new(),
            selected_index: None,
            mode: AppMode::Normal,
            background_tasks: 0,
            notice: None,
            request_state: DirectoryRequestState::Idle,
            cache_age_seconds: None,
            preview_title: String::new(),
            preview_lines: Vec::new(),
            preview_budget: default_preview_budget(),
            task_lines: Vec::new(),
            command_input: String::new(),
            selected_paths: Vec::new(),
            confirm_lines: Vec::new(),
        }
    }

    pub fn set_entries(&mut self, entries: Vec<FileEntry>) {
        self.set_entries_with_selection(entries, None);
    }

    pub fn begin_directory_load(&mut self, path: &str) {
        self.begin_directory_refresh(path, Vec::new(), None, None);
    }

    pub fn finish_directory_load(&mut self, path: &str, result: Result<Vec<FileEntry>, String>) {
        match result {
            Ok(entries) => self.finish_directory_success(path, entries, Some(0), None),
            Err(message) => self.finish_directory_error(path, Vec::new(), None, message, None),
        }
    }

    pub fn begin_directory_refresh(
        &mut self,
        path: &str,
        cached_entries: Vec<FileEntry>,
        cache_age_seconds: Option<u64>,
        preferred_selection_path: Option<&str>,
    ) {
        self.path = normalize_path(path);
        self.cache_age_seconds = cache_age_seconds;
        self.background_tasks = 1;
        self.request_state = if cached_entries.is_empty() {
            DirectoryRequestState::Loading
        } else {
            DirectoryRequestState::Refreshing
        };
        self.notice = Some(match self.request_state {
            DirectoryRequestState::Loading => format!("loading {}", self.path),
            DirectoryRequestState::Refreshing => format!("refreshing {} from cache", self.path),
            DirectoryRequestState::Idle | DirectoryRequestState::Failed => {
                format!("loading {}", self.path)
            }
        });
        self.set_entries_with_selection(cached_entries, preferred_selection_path);
    }

    pub fn finish_directory_success(
        &mut self,
        path: &str,
        entries: Vec<FileEntry>,
        cache_age_seconds: Option<u64>,
        preferred_selection_path: Option<&str>,
    ) {
        self.path = normalize_path(path);
        self.cache_age_seconds = cache_age_seconds;
        self.background_tasks = 0;
        self.request_state = DirectoryRequestState::Idle;
        self.set_entries_with_selection(entries, preferred_selection_path);
        self.notice = Some(format!("loaded {}", self.path));
    }

    pub fn finish_directory_error(
        &mut self,
        path: &str,
        cached_entries: Vec<FileEntry>,
        cache_age_seconds: Option<u64>,
        message: impl Into<String>,
        preferred_selection_path: Option<&str>,
    ) {
        self.path = normalize_path(path);
        self.cache_age_seconds = cache_age_seconds;
        self.background_tasks = 0;
        self.request_state = DirectoryRequestState::Failed;
        self.set_entries_with_selection(cached_entries, preferred_selection_path);
        self.notice = Some(message.into());
    }

    pub fn set_background_tasks(&mut self, count: usize) {
        self.background_tasks = count;
    }

    pub fn set_task_lines(&mut self, lines: Vec<String>) {
        self.task_lines = lines;
    }

    pub fn begin_confirm(&mut self, lines: Vec<String>) {
        self.mode = AppMode::Confirm;
        self.confirm_lines = lines;
    }

    pub fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    pub fn set_cache_age_seconds(&mut self, value: Option<u64>) {
        self.cache_age_seconds = value;
    }

    pub fn begin_preview(&mut self, title: &str, message: &str) {
        self.preview_title = title.to_string();
        self.preview_lines = vec![message.to_string()];
        self.preview_budget = default_preview_budget();
    }

    pub fn finish_preview_text(&mut self, title: &str, content: &str, budget: PreviewBudget) {
        self.preview_title = title.to_string();
        self.preview_lines = content.lines().map(str::to_string).collect();
        self.preview_budget = budget;
    }

    pub fn finish_preview_metadata(
        &mut self,
        title: &str,
        kind: PreviewKind,
        reason: impl Into<String>,
        budget: PreviewBudget,
    ) {
        self.preview_title = title.to_string();
        self.preview_lines = vec![format!("metadata: {}", reason.into())];
        self.preview_budget = PreviewBudget { kind, ..budget };
    }

    pub fn finish_preview_error(&mut self, title: &str, message: impl Into<String>) {
        self.preview_title = title.to_string();
        self.preview_lines = vec![format!("preview_error: {}", message.into())];
        self.preview_budget = default_preview_budget();
    }

    pub fn finish_preview_video(&mut self, title: &str, lines: Vec<String>, budget: PreviewBudget) {
        self.preview_title = title.to_string();
        self.preview_lines = lines;
        self.preview_budget = PreviewBudget {
            kind: PreviewKind::Video,
            ..budget
        };
    }

    pub fn handle_key(&mut self, event: KeyEvent) {
        let _ = self.handle_key_event(event);
    }

    pub fn handle_key_event(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match self.mode {
            AppMode::Normal => self.handle_normal_key(event),
            AppMode::Command => self.handle_command_key(event, None),
            AppMode::Search => self.handle_command_key(event, Some("search")),
            AppMode::Confirm => self.handle_confirm_key(event),
            AppMode::TaskFocus => {
                if event == KeyEvent::Esc {
                    self.mode = AppMode::Normal;
                }
                None
            }
        }
    }

    pub const fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    pub const fn mode(&self) -> AppMode {
        self.mode
    }

    pub const fn request_state(&self) -> DirectoryRequestState {
        self.request_state
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

    pub fn selected_directory_path(&self) -> Option<String> {
        self.selected_entry()
            .filter(|entry| entry.is_dir)
            .map(|entry| normalize_path(entry.path.as_str()))
    }

    pub fn preview_title(&self) -> &str {
        &self.preview_title
    }

    pub fn preview_lines(&self) -> &[String] {
        &self.preview_lines
    }

    pub fn task_lines(&self) -> &[String] {
        &self.task_lines
    }

    pub fn command_input(&self) -> &str {
        &self.command_input
    }

    pub fn selected_paths(&self) -> &[String] {
        &self.selected_paths
    }

    pub fn confirm_lines(&self) -> &[String] {
        &self.confirm_lines
    }

    pub const fn preview_budget(&self) -> PreviewBudget {
        self.preview_budget
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn status_line(&self) -> String {
        format!(
            "mode={} path={} request={} cache_age={} tasks={}",
            self.mode.label(),
            self.path,
            self.request_state.label(),
            self.cache_age_label(),
            self.background_tasks
        )
    }

    pub fn visible_entry_range(&self, rows: usize) -> Range<usize> {
        visible_range(self.entries.len(), self.selected_index.unwrap_or(0), rows)
    }

    pub fn parent_path(path: &str) -> Option<String> {
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() || trimmed == "/" {
            return None;
        }

        match trimmed.rfind('/') {
            Some(0) => Some("/".to_string()),
            Some(index) => Some(trimmed[..index].to_string()),
            None => Some("/".to_string()),
        }
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

    fn handle_normal_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        if event == KeyEvent::Esc {
            self.mode = AppMode::Normal;
            return None;
        }

        match DEFAULT_KEYMAP.action_for(event) {
            KeyAction::Down => self.move_down(),
            KeyAction::Up => self.move_up(),
            KeyAction::Search => {
                self.mode = AppMode::Search;
                self.command_input.clear();
            }
            KeyAction::Command => {
                self.mode = AppMode::Command;
                self.command_input.clear();
            }
            KeyAction::ToggleSelect => self.toggle_selected_path(),
            KeyAction::Back
            | KeyAction::Enter
            | KeyAction::Refresh
            | KeyAction::Preview
            | KeyAction::Download
            | KeyAction::CancelTask
            | KeyAction::Quit => {
                return Some(TuiRuntimeEvent::Action(DEFAULT_KEYMAP.action_for(event)));
            }
            KeyAction::None => {}
        }
        None
    }

    fn handle_command_key(
        &mut self,
        event: KeyEvent,
        submit_prefix: Option<&str>,
    ) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Esc => {
                self.mode = AppMode::Normal;
                self.command_input.clear();
                None
            }
            KeyEvent::Backspace => {
                self.command_input.pop();
                None
            }
            KeyEvent::Enter => {
                let input = self.command_input.trim().to_string();
                self.command_input.clear();
                self.mode = AppMode::Normal;
                if input.is_empty() {
                    None
                } else if let Some(prefix) = submit_prefix {
                    Some(TuiRuntimeEvent::CommandSubmitted(format!(
                        "{prefix} {input}"
                    )))
                } else {
                    Some(TuiRuntimeEvent::CommandSubmitted(input))
                }
            }
            KeyEvent::Char(ch) => {
                self.command_input.push(ch);
                None
            }
        }
    }

    fn handle_confirm_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Char('y') | KeyEvent::Char('Y') => {
                self.mode = AppMode::Normal;
                self.confirm_lines.clear();
                Some(TuiRuntimeEvent::ConfirmAccepted)
            }
            KeyEvent::Char('n') | KeyEvent::Char('N') | KeyEvent::Esc => {
                self.mode = AppMode::Normal;
                self.confirm_lines.clear();
                Some(TuiRuntimeEvent::ConfirmRejected)
            }
            KeyEvent::Enter | KeyEvent::Backspace | KeyEvent::Char(_) => None,
        }
    }

    fn toggle_selected_path(&mut self) {
        let Some(path) = self.selected_entry().map(|entry| entry.path.clone()) else {
            return;
        };
        if let Some(index) = self.selected_paths.iter().position(|item| item == &path) {
            self.selected_paths.remove(index);
        } else {
            self.selected_paths.push(path);
        }
    }

    fn path_is_marked(&self, path: &str) -> bool {
        self.selected_paths.iter().any(|item| item == path)
    }

    fn set_entries_with_selection(
        &mut self,
        mut entries: Vec<FileEntry>,
        preferred_selection_path: Option<&str>,
    ) {
        sort_entries(&mut entries);
        self.entries = entries;
        self.selected_index = preferred_selection_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .or_else(|| (!self.entries.is_empty()).then_some(0));
    }

    fn cache_age_label(&self) -> String {
        self.cache_age_seconds
            .map(|seconds| format!("{seconds}s"))
            .unwrap_or_else(|| "none".to_string())
    }
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|left, right| match right.is_dir.cmp(&left.is_dir) {
        Ordering::Equal => left
            .server_filename
            .to_lowercase()
            .cmp(&right.server_filename.to_lowercase())
            .then_with(|| left.server_filename.cmp(&right.server_filename)),
        order => order,
    });
}

fn visible_range(total: usize, selected: usize, rows: usize) -> Range<usize> {
    if total == 0 || rows == 0 {
        return 0..0;
    }

    let rows = rows.min(total);
    let selected = selected.min(total.saturating_sub(1));
    let scrolloff = rows / 2;
    let start = if selected < scrolloff {
        0
    } else if selected.saturating_add(scrolloff) >= total {
        total.saturating_sub(rows)
    } else {
        selected.saturating_sub(scrolloff)
    };

    start..start.saturating_add(rows)
}

pub fn render_app(frame: &mut Frame<'_>, app: &TuiApp, preview: Option<PreviewBudget>) {
    let [header, body, tasks, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .areas(frame.area());

    frame.render_widget(header_widget(app), header);
    if body.width >= 88 {
        let [locations, files, preview_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22),
                Constraint::Percentage(45),
                Constraint::Min(28),
            ])
            .areas(body);
        frame.render_widget(locations_widget(app), locations);
        frame.render_widget(files_widget(app, files), files);
        frame.render_widget(preview_widget(app, preview), preview_area);
    } else if body.width >= 56 {
        let [files, preview_area] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(54), Constraint::Percentage(46)])
            .areas(body);
        frame.render_widget(files_widget(app, files), files);
        frame.render_widget(preview_widget(app, preview), preview_area);
    } else {
        let [files, preview_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .areas(body);
        frame.render_widget(files_widget(app, files), files);
        frame.render_widget(preview_widget(app, preview), preview_area);
    }
    frame.render_widget(tasks_widget(app), tasks);
    frame.render_widget(status_widget(app), status);
}

pub fn run_terminal_app(app: TuiApp) -> io::Result<()> {
    run_terminal_app_with_handler(app, |_, _| {})
}

pub fn run_terminal_app_with_handler<F>(app: TuiApp, mut handler: F) -> io::Result<()>
where
    F: FnMut(&mut TuiApp, TuiRuntimeEvent),
{
    let mut app = app;
    let mut terminal = ratatui::init();
    let result = run_terminal_loop(&mut terminal, &mut app, &mut handler);
    ratatui::restore();
    result
}

fn run_terminal_loop<F>(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut TuiApp,
    handler: &mut F,
) -> io::Result<()>
where
    F: FnMut(&mut TuiApp, TuiRuntimeEvent),
{
    loop {
        handler(app, TuiRuntimeEvent::Tick);
        terminal.draw(|frame| {
            render_app(frame, app, None);
        })?;

        if event::poll(Duration::from_millis(100))? {
            let CrosstermEvent::Key(key) = event::read()? else {
                continue;
            };
            let Some(tui_key) = map_crossterm_key(key.code) else {
                continue;
            };
            let runtime_event = app.handle_key_event(tui_key);
            if runtime_event == Some(TuiRuntimeEvent::Action(KeyAction::Quit)) {
                break;
            }
            if let Some(runtime_event) = runtime_event {
                handler(app, runtime_event);
            }
        }
    }

    Ok(())
}

fn map_crossterm_key(code: KeyCode) -> Option<KeyEvent> {
    match code {
        KeyCode::Char(value) => Some(KeyEvent::Char(value)),
        KeyCode::Enter => Some(KeyEvent::Enter),
        KeyCode::Backspace => Some(KeyEvent::Backspace),
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
        Span::styled(app.path(), Style::new().fg(Color::White)),
        Span::raw("  "),
        Span::styled(
            app.request_state.label(),
            Style::new().fg(request_state_color(app.request_state)),
        ),
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

fn files_widget(app: &TuiApp, area: Rect) -> List<'_> {
    let rows = area.height.saturating_sub(2) as usize;
    let range = app.visible_entry_range(rows);
    let inner_width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem<'_>> = app.entries()[range.clone()]
        .iter()
        .enumerate()
        .map(|(relative_index, entry)| {
            let index = range.start + relative_index;
            let selected = app.selected_index() == Some(index);
            let marker = if selected {
                ">"
            } else if app.path_is_marked(entry.path.as_str()) {
                "*"
            } else {
                " "
            };
            let suffix = if entry.is_dir { "/" } else { "" };
            let size = file_size_label(entry);
            let name_width = inner_width
                .saturating_sub(UnicodeWidthStr::width(marker))
                .saturating_sub(2)
                .saturating_sub(UnicodeWidthStr::width(size.as_str()))
                .saturating_sub(2)
                .max(4);
            let name = truncate_cells(&format!("{}{}", entry.server_filename, suffix), name_width);
            let base_style = if selected {
                Style::new()
                    .fg(Color::White)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            let name_style = if selected {
                base_style
            } else if entry.is_dir {
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::new().fg(Color::Yellow)),
                Span::raw(" "),
                Span::styled(name, name_style),
                Span::raw("  "),
                Span::styled(size, Style::new().fg(Color::DarkGray)),
            ]))
            .style(base_style)
        })
        .collect();

    let title = format!("Files {} items", app.entries().len());
    List::new(items).block(Block::bordered().title(title))
}

fn preview_widget(app: &TuiApp, preview: Option<PreviewBudget>) -> Paragraph<'_> {
    let selected = app.selected_entry();
    let title = if app.preview_title.is_empty() {
        selected
            .map(|entry| entry.server_filename.as_str())
            .or_else(|| app.notice())
            .unwrap_or("metadata")
    } else {
        app.preview_title.as_str()
    };
    let kind = selected
        .map(|entry| if entry.is_dir { "directory" } else { "file" })
        .unwrap_or("none");
    let budget = preview.unwrap_or(app.preview_budget);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("selected=", Style::new().fg(Color::DarkGray)),
            Span::raw(truncate_cells(title, 24)),
        ]),
        Line::from(vec![
            Span::styled("kind=", Style::new().fg(Color::DarkGray)),
            Span::raw(kind.to_string()),
        ]),
        Line::from(vec![
            Span::styled("path=", Style::new().fg(Color::DarkGray)),
            Span::raw(truncate_cells(
                selected
                    .map(|entry| entry.path.as_str())
                    .unwrap_or_else(|| app.path()),
                24,
            )),
        ]),
        Line::raw(""),
        Line::raw(format!("preview={}", budget.kind.label())),
        Line::raw(format!("fetched={}", budget.fetched_bytes)),
        Line::raw(format!("limit={}", budget.memory_limit_bytes)),
        Line::raw(format!("truncated={}", budget.truncated)),
        Line::raw(format!("disk={}", budget.disk_backed)),
        Line::raw(""),
    ];
    for line in app.preview_lines.iter().take(6) {
        lines.push(Line::raw(line.to_string()));
    }

    Paragraph::new(lines)
        .block(Block::bordered().title("Preview"))
        .wrap(Wrap { trim: true })
}

fn default_preview_budget() -> PreviewBudget {
    PreviewBudget {
        kind: PreviewKind::Metadata,
        fetched_bytes: 0,
        memory_limit_bytes: 16 * 1024 * 1024,
        truncated: false,
        disk_backed: false,
    }
}

fn tasks_widget(app: &TuiApp) -> Paragraph<'_> {
    if !app.confirm_lines.is_empty() {
        let mut lines = Vec::new();
        if let Some(summary) = app.confirm_lines.first() {
            lines.push(Line::raw(summary.to_string()));
        }
        if let Some(prompt) = app.confirm_lines.last() {
            if app.confirm_lines.first() != Some(prompt) {
                lines.push(Line::raw(prompt.to_string()));
            }
        }
        return Paragraph::new(lines).block(Block::bordered().title("Tasks"));
    }

    let mut lines = vec![Line::raw(format!(
        "request={} cache_age={} background={}",
        app.request_state.label(),
        app.cache_age_label(),
        app.background_tasks,
    ))];
    if !app.command_input.is_empty() || matches!(app.mode, AppMode::Command | AppMode::Search) {
        let prompt = if app.mode == AppMode::Search {
            "/"
        } else {
            ":"
        };
        lines.push(Line::raw(format!("{prompt}{}", app.command_input)));
    }
    for line in app.task_lines.iter().take(3) {
        lines.push(Line::raw(line.to_string()));
    }
    if let Some(notice) = app.notice() {
        lines.push(Line::raw(notice.to_string()));
    }
    Paragraph::new(lines).block(Block::bordered().title("Tasks"))
}

fn status_widget(app: &TuiApp) -> Paragraph<'_> {
    Paragraph::new(Line::raw(format!(
        "Status {}  keys j/k h/l space / : r d/x p q",
        app.status_line()
    )))
    .style(Style::new().fg(Color::Black).bg(Color::Gray))
}

fn request_state_color(state: DirectoryRequestState) -> Color {
    match state {
        DirectoryRequestState::Idle => Color::Green,
        DirectoryRequestState::Loading | DirectoryRequestState::Refreshing => Color::Yellow,
        DirectoryRequestState::Failed => Color::Red,
    }
}

fn file_size_label(entry: &FileEntry) -> String {
    if entry.is_dir {
        return "<DIR>".to_string();
    }

    let bytes = u64::try_from(entry.size).unwrap_or(0);
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{}KiB", bytes / 1024)
    } else {
        format!("{}MiB", bytes / 1024 / 1024)
    }
}

fn truncate_cells(value: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let budget = max_width - 3;
    let mut output = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + width > budget {
            break;
        }
        output.push(ch);
        used += width;
    }
    output.push_str("...");
    output
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
