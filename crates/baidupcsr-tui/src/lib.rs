use baidupcsr_pcs::FileEntry;
use ratatui::{
    crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyCode,
            KeyModifiers, MouseButton, MouseEventKind,
        },
        execute,
    },
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Wrap,
    },
    Frame,
};
use std::{cmp::Ordering, io, ops::Range, time::Duration};
use time::{macros::format_description, OffsetDateTime};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const fn crate_name() -> &'static str {
    "baidupcsr-tui"
}

pub const DEFAULT_KEYMAP: KeyMap = KeyMap;

const THEME_BG: Color = Color::Rgb(15, 23, 42);
const THEME_PANEL: Color = Color::Rgb(24, 32, 52);
const THEME_BORDER: Color = Color::Rgb(51, 65, 85);
const THEME_BORDER_ACTIVE: Color = Color::Rgb(34, 211, 238);
const THEME_TEXT: Color = Color::Rgb(226, 232, 240);
const THEME_STATUS_TEXT: Color = Color::Rgb(203, 213, 225);
const THEME_MUTED: Color = Color::Rgb(148, 163, 184);
const THEME_DIM: Color = Color::Rgb(100, 116, 139);
const THEME_ACCENT: Color = Color::Rgb(94, 234, 212);
const THEME_SELECTED: Color = Color::Rgb(14, 116, 144);
const THEME_MARKED: Color = Color::Rgb(30, 41, 59);
const THEME_DIRECTORY: Color = Color::Rgb(125, 211, 252);
const THEME_WARNING: Color = Color::Rgb(250, 204, 21);
const THEME_SUCCESS: Color = Color::Rgb(52, 211, 153);
const THEME_ERROR: Color = Color::Rgb(248, 113, 113);
const PAGE_STEP: usize = 10;
const HALF_PAGE_STEP: usize = 5;
const MOUSE_WHEEL_STEP: usize = 3;
const DOUBLE_CLICK_TICKS: u64 = 5;
const MARQUEE_EDGE_PAUSE_TICKS: u64 = 8;
const HEADER_BRAND: &str = "baidupcsr";
const HEADER_GAP: &str = "  ";
const BREADCRUMB_SEPARATOR: &str = " > ";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppMode {
    Normal,
    Filter,
    Command,
    Confirm,
    TaskFocus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanelFocus {
    Files,
    Preview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfirmKind {
    Operation,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileClick {
    index: usize,
    tick: u64,
}

impl AppMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Filter => "FILTER",
            Self::Command => "COMMAND",
            Self::Confirm => "CONFIRM",
            Self::TaskFocus => "TASKS",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyEvent {
    Char(char),
    Ctrl(char),
    Tab,
    Enter,
    Backspace,
    Esc,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
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
    Select,
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    Top,
    Bottom,
    Quit,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseInputKind {
    LeftClick,
    DoubleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MouseInput {
    pub kind: MouseInputKind,
    pub column: u16,
    pub row: u16,
    pub area: Rect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TuiRuntimeEvent {
    Tick,
    Action(KeyAction),
    CommandSubmitted(String),
    PathRequested(String),
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
    const fn user_label(self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::Loading => "Loading",
            Self::Refreshing => "Refreshing",
            Self::Failed => "Error",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheSource {
    None,
    Memory,
    Disk,
    Network,
}

impl CacheSource {
    const fn user_label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Memory => "memory",
            Self::Disk => "disk",
            Self::Network => "network",
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
            KeyEvent::Down => KeyAction::Down,
            KeyEvent::Up => KeyAction::Up,
            KeyEvent::Left => KeyAction::Back,
            KeyEvent::Right => KeyAction::Enter,
            KeyEvent::PageUp => KeyAction::PageUp,
            KeyEvent::PageDown => KeyAction::PageDown,
            KeyEvent::Home => KeyAction::Top,
            KeyEvent::End => KeyAction::Bottom,
            KeyEvent::Ctrl('u') | KeyEvent::Ctrl('U') => KeyAction::HalfPageUp,
            KeyEvent::Ctrl('d') | KeyEvent::Ctrl('D') => KeyAction::HalfPageDown,
            KeyEvent::Char('/') => KeyAction::Search,
            KeyEvent::Char(':') => KeyAction::Command,
            KeyEvent::Char('G') => KeyAction::Bottom,
            KeyEvent::Char('r') => KeyAction::Refresh,
            KeyEvent::Char('p') => KeyAction::Preview,
            KeyEvent::Char('d') => KeyAction::Download,
            KeyEvent::Char('x') => KeyAction::CancelTask,
            KeyEvent::Char(' ') => KeyAction::ToggleSelect,
            KeyEvent::Char('q') => KeyAction::Quit,
            KeyEvent::Esc => KeyAction::None,
            KeyEvent::Enter | KeyEvent::Backspace | KeyEvent::Tab => KeyAction::None,
            KeyEvent::Ctrl(_) => KeyAction::None,
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
    cache_source: CacheSource,
    preview_title: String,
    preview_lines: Vec<String>,
    preview_budget: PreviewBudget,
    task_lines: Vec<String>,
    preload_lines: Vec<String>,
    command_input: String,
    filter_input: String,
    selected_paths: Vec<String>,
    confirm_lines: Vec<String>,
    confirm_kind: Option<ConfirmKind>,
    help_visible: bool,
    pending_g: bool,
    scroll_offset: usize,
    preview_scroll_offset: usize,
    marquee_tick: u64,
    last_file_click: Option<FileClick>,
    focus: PanelFocus,
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
            cache_source: CacheSource::None,
            preview_title: String::new(),
            preview_lines: Vec::new(),
            preview_budget: default_preview_budget(),
            task_lines: Vec::new(),
            preload_lines: Vec::new(),
            command_input: String::new(),
            filter_input: String::new(),
            selected_paths: Vec::new(),
            confirm_lines: Vec::new(),
            confirm_kind: None,
            help_visible: false,
            pending_g: false,
            scroll_offset: 0,
            preview_scroll_offset: 0,
            marquee_tick: 0,
            last_file_click: None,
            focus: PanelFocus::Files,
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
        let path = normalize_path(path);
        if path != self.path {
            self.filter_input.clear();
        }
        self.path = path;
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

    pub fn set_preload_lines(&mut self, lines: Vec<String>) {
        self.preload_lines = lines;
    }

    pub fn begin_confirm(&mut self, lines: Vec<String>) {
        self.mode = AppMode::Confirm;
        self.confirm_lines = lines;
        self.confirm_kind = Some(ConfirmKind::Operation);
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

    pub fn set_cache_source(&mut self, source: CacheSource) {
        self.cache_source = source;
    }

    pub fn begin_preview(&mut self, title: &str, message: &str) {
        self.preview_title = title.to_string();
        self.preview_lines = vec![message.to_string()];
        self.preview_budget = default_preview_budget();
        self.preview_scroll_offset = 0;
    }

    pub fn finish_preview_text(&mut self, title: &str, content: &str, budget: PreviewBudget) {
        self.preview_title = title.to_string();
        self.preview_lines = content.lines().map(str::to_string).collect();
        self.preview_budget = budget;
        self.preview_scroll_offset = 0;
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
        self.preview_scroll_offset = 0;
    }

    pub fn finish_preview_error(&mut self, title: &str, message: impl Into<String>) {
        self.preview_title = title.to_string();
        self.preview_lines = vec![format!("preview_error: {}", message.into())];
        self.preview_budget = default_preview_budget();
        self.preview_scroll_offset = 0;
    }

    pub fn finish_preview_video(&mut self, title: &str, lines: Vec<String>, budget: PreviewBudget) {
        self.preview_title = title.to_string();
        self.preview_lines = lines;
        self.preview_budget = PreviewBudget {
            kind: PreviewKind::Video,
            ..budget
        };
        self.preview_scroll_offset = 0;
    }

    pub fn finish_preview_image_metadata(
        &mut self,
        title: &str,
        lines: Vec<String>,
        budget: PreviewBudget,
    ) {
        self.preview_title = title.to_string();
        self.preview_lines = lines;
        self.preview_budget = PreviewBudget {
            kind: PreviewKind::Image,
            ..budget
        };
        self.preview_scroll_offset = 0;
    }

    pub fn handle_key(&mut self, event: KeyEvent) {
        let _ = self.handle_key_event(event);
    }

    pub fn handle_mouse_event(&mut self, input: MouseInput) -> Option<TuiRuntimeEvent> {
        if self.help_visible {
            if matches!(
                input.kind,
                MouseInputKind::LeftClick | MouseInputKind::DoubleClick
            ) {
                self.help_visible = false;
            }
            return None;
        }

        let full = full_layout(input.area);
        let layout = browser_layout(full.body);
        let files_inner = inner_rect(layout.files);
        let preview_inner = inner_rect(layout.preview);

        match input.kind {
            MouseInputKind::ScrollUp => {
                if rect_contains(files_inner, input.column, input.row) {
                    self.focus = PanelFocus::Files;
                    return self
                        .scroll_files_by_wheel(-(MOUSE_WHEEL_STEP as isize))
                        .then_some(TuiRuntimeEvent::Action(KeyAction::Select));
                }
                if rect_contains(preview_inner, input.column, input.row) {
                    self.focus = PanelFocus::Preview;
                    self.scroll_preview_by(
                        -(MOUSE_WHEEL_STEP as isize),
                        usize::from(preview_inner.height),
                    );
                }
                None
            }
            MouseInputKind::ScrollDown => {
                if rect_contains(files_inner, input.column, input.row) {
                    self.focus = PanelFocus::Files;
                    return self
                        .scroll_files_by_wheel(MOUSE_WHEEL_STEP as isize)
                        .then_some(TuiRuntimeEvent::Action(KeyAction::Select));
                }
                if rect_contains(preview_inner, input.column, input.row) {
                    self.focus = PanelFocus::Preview;
                    self.scroll_preview_by(
                        MOUSE_WHEEL_STEP as isize,
                        usize::from(preview_inner.height),
                    );
                }
                None
            }
            MouseInputKind::LeftClick | MouseInputKind::DoubleClick => {
                if let Some(path) =
                    breadcrumb_path_at(self.path(), full.header, input.column, input.row)
                {
                    return (path.as_str() != self.path.as_str())
                        .then_some(TuiRuntimeEvent::PathRequested(path));
                }

                if rect_contains(preview_inner, input.column, input.row) {
                    self.focus = PanelFocus::Preview;
                    return None;
                }

                if !rect_contains(files_inner, input.column, input.row) {
                    return None;
                }
                self.focus = PanelFocus::Files;

                let row_offset = usize::from(input.row.saturating_sub(files_inner.y));
                let visible_indices = self.filtered_indices();
                let range = self.visible_entry_range(usize::from(files_inner.height));
                let visible_index = range.start.saturating_add(row_offset);
                let index = visible_indices.get(visible_index).copied()?;
                let is_double_click = input.kind == MouseInputKind::DoubleClick
                    || self.last_file_click.is_some_and(|click| {
                        click.index == index
                            && self.marquee_tick.saturating_sub(click.tick) <= DOUBLE_CLICK_TICKS
                    });
                self.select_index(Some(index));
                if is_double_click {
                    self.last_file_click = None;
                    if self.selected_entry().is_some_and(|entry| entry.is_dir) {
                        Some(TuiRuntimeEvent::Action(KeyAction::Enter))
                    } else {
                        Some(TuiRuntimeEvent::Action(KeyAction::Preview))
                    }
                } else {
                    self.last_file_click = Some(FileClick {
                        index,
                        tick: self.marquee_tick,
                    });
                    Some(TuiRuntimeEvent::Action(KeyAction::Select))
                }
            }
        }
    }

    pub fn advance_tick(&mut self) {
        self.marquee_tick = self.marquee_tick.wrapping_add(1);
    }

    pub fn handle_key_event(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        if self.help_visible {
            if matches!(event, KeyEvent::Char('?') | KeyEvent::Esc) {
                self.help_visible = false;
            }
            return None;
        }

        match self.mode {
            AppMode::Normal => self.handle_normal_key(event),
            AppMode::Command => self.handle_command_key(event),
            AppMode::Filter => self.handle_filter_key(event),
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

    pub const fn help_visible(&self) -> bool {
        self.help_visible
    }

    pub const fn focus(&self) -> PanelFocus {
        self.focus
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

    pub fn preload_lines(&self) -> &[String] {
        &self.preload_lines
    }

    pub fn command_input(&self) -> &str {
        &self.command_input
    }

    pub fn filter_input(&self) -> &str {
        &self.filter_input
    }

    pub fn filtered_entry_count(&self) -> usize {
        self.filtered_indices().len()
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

    pub const fn preview_scroll_offset(&self) -> usize {
        self.preview_scroll_offset
    }

    pub const fn cache_source(&self) -> CacheSource {
        self.cache_source
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn status_line(&self) -> String {
        let mut parts = vec![
            self.mode.label().to_string(),
            self.position_label(),
            format!("{} selected", self.selected_paths.len()),
            self.path.clone(),
        ];
        if let Some(hint) = self.status_hint() {
            parts.push(hint);
        }
        parts.join("  ")
    }

    pub fn visible_entry_range(&self, rows: usize) -> Range<usize> {
        let indices = self.filtered_indices();
        visible_range_from_offset(indices.len(), self.scroll_offset, rows)
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
        self.move_visible_by(1);
    }

    fn move_up(&mut self) {
        self.move_visible_by(-1);
    }

    fn move_page_down(&mut self) {
        self.move_visible_by(PAGE_STEP as isize);
    }

    fn move_page_up(&mut self) {
        self.move_visible_by(-(PAGE_STEP as isize));
    }

    fn move_half_page_down(&mut self) {
        self.move_visible_by(HALF_PAGE_STEP as isize);
    }

    fn move_half_page_up(&mut self) {
        self.move_visible_by(-(HALF_PAGE_STEP as isize));
    }

    fn move_to_top(&mut self) {
        let indices = self.filtered_indices();
        let Some(index) = indices.first().copied() else {
            return;
        };
        self.select_index(Some(index));
        self.scroll_offset = 0;
    }

    fn move_to_bottom(&mut self) {
        let indices = self.filtered_indices();
        let Some(index) = indices.last().copied() else {
            return;
        };
        self.select_index(Some(index));
        self.scroll_offset = indices.len().saturating_sub(1);
    }

    fn move_visible_by(&mut self, delta: isize) {
        let indices = self.filtered_indices();
        let Some(position) = self.selected_visible_position(&indices) else {
            return;
        };
        let last = indices.len().saturating_sub(1);
        let next = if delta.is_negative() {
            position.saturating_sub(delta.unsigned_abs())
        } else {
            position.saturating_add(delta as usize).min(last)
        };
        self.select_index(indices.get(next).copied());
        self.scroll_offset = next.saturating_sub(1);
    }

    fn scroll_files_by_wheel(&mut self, delta: isize) -> bool {
        let old_offset = self.scroll_offset;
        let old_selection = self.selected_index;
        self.move_visible_by(delta);
        old_offset != self.scroll_offset || old_selection != self.selected_index
    }

    fn scroll_preview_by(&mut self, delta: isize, rows: usize) -> bool {
        let old_offset = self.preview_scroll_offset;
        let total = self.preview_render_line_count();
        if total == 0 || rows == 0 {
            self.preview_scroll_offset = 0;
            return old_offset != self.preview_scroll_offset;
        }
        let last_start = total.saturating_sub(rows.min(total));
        self.preview_scroll_offset = if delta.is_negative() {
            self.preview_scroll_offset
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.preview_scroll_offset
                .saturating_add(delta as usize)
                .min(last_start)
        };
        old_offset != self.preview_scroll_offset
    }

    fn select_index(&mut self, index: Option<usize>) {
        if self.selected_index != index {
            self.preview_scroll_offset = 0;
            self.marquee_tick = 0;
            self.last_file_click = None;
        }
        self.selected_index = index;
    }

    fn preview_render_line_count(&self) -> usize {
        let detail_count = if self.selected_entry().is_some() {
            8
        } else {
            3
        };
        if self.preview_budget.kind == PreviewKind::Text && !self.preview_lines.is_empty() {
            return 4 + 2 + self.preview_lines.len();
        }
        if self.preview_budget.kind == PreviewKind::Video && !self.preview_lines.is_empty() {
            return detail_count + 3 + self.preview_lines.len();
        }
        detail_count + 2 + self.preview_lines.len()
    }

    fn handle_normal_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        if event == KeyEvent::Char('?') {
            self.help_visible = true;
            return None;
        }

        if event == KeyEvent::Tab {
            self.toggle_focus();
            return None;
        }

        if self.pending_g {
            self.pending_g = false;
            if event == KeyEvent::Char('g') {
                self.move_to_top();
                return Some(TuiRuntimeEvent::Action(KeyAction::Top));
            }
        } else if event == KeyEvent::Char('g') {
            self.pending_g = true;
            return None;
        }

        if event == KeyEvent::Esc {
            self.mode = AppMode::Normal;
            self.pending_g = false;
            return None;
        }

        match DEFAULT_KEYMAP.action_for(event) {
            KeyAction::Down => {
                self.move_down();
                return Some(TuiRuntimeEvent::Action(KeyAction::Down));
            }
            KeyAction::Up => {
                self.move_up();
                return Some(TuiRuntimeEvent::Action(KeyAction::Up));
            }
            KeyAction::PageDown => {
                self.move_page_down();
                return Some(TuiRuntimeEvent::Action(KeyAction::PageDown));
            }
            KeyAction::PageUp => {
                self.move_page_up();
                return Some(TuiRuntimeEvent::Action(KeyAction::PageUp));
            }
            KeyAction::HalfPageDown => {
                self.move_half_page_down();
                return Some(TuiRuntimeEvent::Action(KeyAction::HalfPageDown));
            }
            KeyAction::HalfPageUp => {
                self.move_half_page_up();
                return Some(TuiRuntimeEvent::Action(KeyAction::HalfPageUp));
            }
            KeyAction::Top => {
                self.move_to_top();
                return Some(TuiRuntimeEvent::Action(KeyAction::Top));
            }
            KeyAction::Bottom => {
                self.move_to_bottom();
                return Some(TuiRuntimeEvent::Action(KeyAction::Bottom));
            }
            KeyAction::Search => {
                self.mode = AppMode::Filter;
                self.filter_input.clear();
                self.sync_selection_to_filter();
            }
            KeyAction::Command => {
                self.mode = AppMode::Command;
                self.command_input.clear();
            }
            KeyAction::Quit => {
                if self.background_tasks > 0 {
                    self.begin_quit_confirm();
                } else {
                    return Some(TuiRuntimeEvent::Action(KeyAction::Quit));
                }
            }
            KeyAction::ToggleSelect => self.toggle_selected_path(),
            KeyAction::Back
            | KeyAction::Enter
            | KeyAction::Refresh
            | KeyAction::Preview
            | KeyAction::Download
            | KeyAction::CancelTask
            | KeyAction::Select => {
                return Some(TuiRuntimeEvent::Action(DEFAULT_KEYMAP.action_for(event)));
            }
            KeyAction::None => {}
        }
        None
    }

    fn handle_command_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
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
                } else {
                    Some(TuiRuntimeEvent::CommandSubmitted(input))
                }
            }
            KeyEvent::Char(ch) => {
                self.command_input.push(ch);
                None
            }
            KeyEvent::Up
            | KeyEvent::Down
            | KeyEvent::Left
            | KeyEvent::Right
            | KeyEvent::PageUp
            | KeyEvent::PageDown
            | KeyEvent::Home
            | KeyEvent::End
            | KeyEvent::Ctrl(_)
            | KeyEvent::Tab => None,
        }
    }

    fn handle_filter_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Esc => {
                self.mode = AppMode::Normal;
                self.filter_input.clear();
                self.sync_selection_to_filter();
                None
            }
            KeyEvent::Backspace => {
                self.filter_input.pop();
                self.sync_selection_to_filter();
                None
            }
            KeyEvent::Enter => {
                self.mode = AppMode::Normal;
                None
            }
            KeyEvent::Char(ch) => {
                self.filter_input.push(ch);
                self.sync_selection_to_filter();
                None
            }
            KeyEvent::Up
            | KeyEvent::Down
            | KeyEvent::Left
            | KeyEvent::Right
            | KeyEvent::PageUp
            | KeyEvent::PageDown
            | KeyEvent::Home
            | KeyEvent::End
            | KeyEvent::Ctrl(_)
            | KeyEvent::Tab => None,
        }
    }

    fn handle_confirm_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Char('y') | KeyEvent::Char('Y') => {
                let kind = self.confirm_kind.take().unwrap_or(ConfirmKind::Operation);
                self.mode = AppMode::Normal;
                self.confirm_lines.clear();
                match kind {
                    ConfirmKind::Operation => Some(TuiRuntimeEvent::ConfirmAccepted),
                    ConfirmKind::Quit => Some(TuiRuntimeEvent::Action(KeyAction::Quit)),
                }
            }
            KeyEvent::Char('n') | KeyEvent::Char('N') | KeyEvent::Esc => {
                let kind = self.confirm_kind.take().unwrap_or(ConfirmKind::Operation);
                self.mode = AppMode::Normal;
                self.confirm_lines.clear();
                match kind {
                    ConfirmKind::Operation => Some(TuiRuntimeEvent::ConfirmRejected),
                    ConfirmKind::Quit => None,
                }
            }
            KeyEvent::Enter
            | KeyEvent::Backspace
            | KeyEvent::Char(_)
            | KeyEvent::Up
            | KeyEvent::Down
            | KeyEvent::Left
            | KeyEvent::Right
            | KeyEvent::PageUp
            | KeyEvent::PageDown
            | KeyEvent::Home
            | KeyEvent::End
            | KeyEvent::Ctrl(_)
            | KeyEvent::Tab => None,
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
        self.last_file_click = None;
        let selected_index = preferred_selection_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .or_else(|| (!self.entries.is_empty()).then_some(0));
        self.select_index(selected_index);
        self.sync_selection_to_filter();
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let needle = self.filter_input.trim().to_lowercase();
        if needle.is_empty() {
            return (0..self.entries.len()).collect();
        }
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                entry
                    .server_filename
                    .to_lowercase()
                    .contains(needle.as_str())
                    .then_some(index)
            })
            .collect()
    }

    fn selected_visible_position(&self, indices: &[usize]) -> Option<usize> {
        let selected = self.selected_index?;
        indices.iter().position(|index| *index == selected)
    }

    fn sync_selection_to_filter(&mut self) {
        let indices = self.filtered_indices();
        if indices.is_empty() {
            self.select_index(None);
            self.scroll_offset = 0;
            return;
        }
        if let Some(position) = self.selected_visible_position(&indices) {
            self.scroll_offset = position.saturating_sub(1);
            return;
        }
        self.select_index(indices.first().copied());
        self.scroll_offset = 0;
    }

    fn begin_quit_confirm(&mut self) {
        self.mode = AppMode::Confirm;
        self.confirm_kind = Some(ConfirmKind::Quit);
        self.confirm_lines = vec![format!(
            "Quit with {} {} still running?",
            self.background_tasks,
            pluralize(self.background_tasks, "task", "tasks")
        )];
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            PanelFocus::Files => PanelFocus::Preview,
            PanelFocus::Preview => PanelFocus::Files,
        };
    }

    fn position_label(&self) -> String {
        let indices = self.filtered_indices();
        if indices.is_empty() {
            return "0/0".to_string();
        }
        let position = self.selected_visible_position(&indices).unwrap_or(0);
        format!("{}/{}", position.saturating_add(1), indices.len())
    }

    fn status_hint(&self) -> Option<String> {
        match self.request_state {
            DirectoryRequestState::Loading => Some("Loading...".to_string()),
            DirectoryRequestState::Refreshing => Some("Refreshing...".to_string()),
            DirectoryRequestState::Failed => Some(
                self.notice
                    .as_ref()
                    .map(|notice| format!("Error: {notice}"))
                    .unwrap_or_else(|| "Error".to_string()),
            ),
            DirectoryRequestState::Idle if !self.filter_input.is_empty() => {
                Some(format!("filter \"{}\"", self.filter_input))
            }
            DirectoryRequestState::Idle if self.background_tasks > 0 => Some(format!(
                "{} {}",
                self.background_tasks,
                pluralize(self.background_tasks, "task", "tasks")
            )),
            DirectoryRequestState::Idle => None,
        }
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

fn visible_range_from_offset(total: usize, offset: usize, rows: usize) -> Range<usize> {
    if total == 0 || rows == 0 {
        return 0..0;
    }

    let rows = rows.min(total);
    let start = offset.min(total.saturating_sub(rows));

    start..start.saturating_add(rows)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TuiLayout {
    files: Rect,
    preview: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FullLayout {
    header: Rect,
    body: Rect,
    tasks: Rect,
    status: Rect,
}

fn full_layout(area: Rect) -> FullLayout {
    let [header, body, tasks, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .areas(area);
    FullLayout {
        header,
        body,
        tasks,
        status,
    }
}

fn browser_layout(body: Rect) -> TuiLayout {
    if body.width >= 88 {
        let [files, preview] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Min(42)])
            .areas(body);
        TuiLayout { files, preview }
    } else if body.width >= 56 {
        let [files, preview] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(54), Constraint::Percentage(46)])
            .areas(body);
        TuiLayout { files, preview }
    } else {
        let [files, preview] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .areas(body);
        TuiLayout { files, preview }
    }
}

fn inner_rect(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

pub fn render_app(frame: &mut Frame<'_>, app: &TuiApp, preview: Option<PreviewBudget>) {
    frame.render_widget(Block::new().style(surface_style()), frame.area());

    let layout = full_layout(frame.area());
    let browser = browser_layout(layout.body);
    frame.render_widget(header_widget(app), layout.header);
    render_files_pane(frame, app, browser.files);
    frame.render_widget(
        preview_widget(app, preview, browser.preview),
        browser.preview,
    );
    frame.render_widget(tasks_widget(app), layout.tasks);
    frame.render_widget(status_widget(app), layout.status);
    if app.help_visible() {
        let area = centered_rect(frame.area(), 58, 22);
        frame.render_widget(Clear, area);
        frame.render_widget(help_widget(), area);
    }
    if !app.confirm_lines().is_empty() {
        let area = confirm_modal_rect(frame.area(), app);
        frame.render_widget(Clear, area);
        frame.render_widget(confirm_widget(app), area);
    }
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
    execute!(io::stdout(), EnableMouseCapture)?;
    let result = run_terminal_loop(&mut terminal, &mut app, &mut handler);
    let disable_mouse = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result.and(disable_mouse)
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
        app.advance_tick();
        handler(app, TuiRuntimeEvent::Tick);
        terminal.draw(|frame| {
            render_app(frame, app, None);
        })?;

        if event::poll(Duration::from_millis(100))? {
            let runtime_event = match event::read()? {
                CrosstermEvent::Key(key) => {
                    let Some(tui_key) = map_crossterm_key(key.code, key.modifiers) else {
                        continue;
                    };
                    app.handle_key_event(tui_key)
                }
                CrosstermEvent::Mouse(mouse) => {
                    let Some(kind) = map_crossterm_mouse_kind(mouse.kind) else {
                        continue;
                    };
                    let size = terminal.size()?;
                    app.handle_mouse_event(MouseInput {
                        kind,
                        column: mouse.column,
                        row: mouse.row,
                        area: Rect::new(0, 0, size.width, size.height),
                    })
                }
                _ => continue,
            };
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

fn map_crossterm_mouse_kind(kind: MouseEventKind) -> Option<MouseInputKind> {
    match kind {
        MouseEventKind::Down(MouseButton::Left) => Some(MouseInputKind::LeftClick),
        MouseEventKind::ScrollUp => Some(MouseInputKind::ScrollUp),
        MouseEventKind::ScrollDown => Some(MouseInputKind::ScrollDown),
        _ => None,
    }
}

fn map_crossterm_key(code: KeyCode, modifiers: KeyModifiers) -> Option<KeyEvent> {
    match code {
        KeyCode::Char(value) if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(KeyEvent::Ctrl(value))
        }
        KeyCode::Char(value) => Some(KeyEvent::Char(value)),
        KeyCode::Enter => Some(KeyEvent::Enter),
        KeyCode::Backspace => Some(KeyEvent::Backspace),
        KeyCode::Tab => Some(KeyEvent::Tab),
        KeyCode::Esc => Some(KeyEvent::Esc),
        KeyCode::Up => Some(KeyEvent::Up),
        KeyCode::Down => Some(KeyEvent::Down),
        KeyCode::Left => Some(KeyEvent::Left),
        KeyCode::Right => Some(KeyEvent::Right),
        KeyCode::PageUp => Some(KeyEvent::PageUp),
        KeyCode::PageDown => Some(KeyEvent::PageDown),
        KeyCode::Home => Some(KeyEvent::Home),
        KeyCode::End => Some(KeyEvent::End),
        _ => None,
    }
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn confirm_modal_rect(area: Rect, app: &TuiApp) -> Rect {
    let body_lines = confirm_body_lines(app).len() as u16;
    centered_rect(area, 64, body_lines.saturating_add(4).clamp(7, 14))
}

fn surface_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_BG)
}

fn content_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_PANEL)
}

fn meta_label_style() -> Style {
    Style::new().fg(THEME_MUTED).bg(THEME_PANEL)
}

fn meta_value_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_PANEL)
}

fn preload_style() -> Style {
    Style::new().fg(THEME_DIRECTORY).bg(THEME_PANEL)
}

fn notice_style() -> Style {
    Style::new().fg(THEME_WARNING).bg(THEME_PANEL)
}

fn warning_style() -> Style {
    Style::new()
        .fg(THEME_WARNING)
        .bg(THEME_PANEL)
        .add_modifier(Modifier::BOLD)
}

fn confirm_style() -> Style {
    Style::new()
        .fg(THEME_ERROR)
        .bg(THEME_PANEL)
        .add_modifier(Modifier::BOLD)
}

fn title_style(active: bool) -> Style {
    let color = if active {
        THEME_ACCENT
    } else {
        THEME_DIRECTORY
    };
    Style::new()
        .fg(color)
        .bg(THEME_BG)
        .add_modifier(Modifier::BOLD)
}

fn themed_block(title: impl Into<String>, active: bool) -> Block<'static> {
    let border = if active {
        THEME_BORDER_ACTIVE
    } else {
        THEME_BORDER
    };
    Block::bordered()
        .style(surface_style())
        .border_style(Style::new().fg(border).bg(THEME_BG))
        .title(Line::from(Span::styled(title.into(), title_style(active))))
}

fn detail_value_width(inner_width: usize, label: &str) -> usize {
    inner_width
        .saturating_sub(UnicodeWidthStr::width(label))
        .saturating_sub(1)
        .max(1)
}

fn detail_line(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(label, meta_label_style()),
        Span::styled(" ", surface_style()),
        Span::styled(value, meta_value_style()),
    ])
}

fn content_line(value: impl Into<String>) -> Line<'static> {
    Line::raw(value.into()).style(content_style())
}

fn help_section_line(value: &'static str) -> Line<'static> {
    Line::raw(value).style(
        Style::new()
            .fg(THEME_ACCENT)
            .bg(THEME_PANEL)
            .add_modifier(Modifier::BOLD),
    )
}

fn help_widget() -> Paragraph<'static> {
    Paragraph::new(vec![
        help_section_line("Navigation"),
        content_line("  j/k Up/Down        move selection"),
        content_line("  gg/G Home/End      first / last"),
        content_line("  PgUp/PgDn C-u/C-d  page / half-page"),
        content_line("  h/l Left/Right     parent / enter"),
        content_line("  Tab                switch pane focus"),
        content_line("  Enter              submit input"),
        Line::raw("").style(surface_style()),
        help_section_line("Actions"),
        content_line("  / filter           : command"),
        content_line("  ? help             Esc close/cancel"),
        content_line("  space mark         r refresh"),
        content_line("  d download         x cancel task"),
        content_line("  p preview/open     q quit"),
        content_line("  y/n                confirm / cancel"),
        Line::raw("").style(surface_style()),
        help_section_line("Mouse"),
        content_line("  file click selects double-click opens"),
        content_line("  wheel follows mouse pane"),
        content_line("  path click jumps"),
    ])
    .block(themed_block("Shortcuts", true))
    .style(surface_style())
    .wrap(Wrap { trim: true })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BreadcrumbSegment {
    label: String,
    target: String,
}

fn breadcrumb_segments(path: &str) -> Vec<BreadcrumbSegment> {
    let normalized = normalize_path(path);
    let mut segments = vec![BreadcrumbSegment {
        label: "/".to_string(),
        target: "/".to_string(),
    }];
    let mut target = String::new();
    for part in normalized
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
    {
        target.push('/');
        target.push_str(part);
        segments.push(BreadcrumbSegment {
            label: part.to_string(),
            target: target.clone(),
        });
    }
    segments
}

fn breadcrumb_spans(path: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, segment) in breadcrumb_segments(path).into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                BREADCRUMB_SEPARATOR,
                Style::new().fg(THEME_DIM).bg(THEME_BG),
            ));
        }
        spans.push(Span::styled(
            format!(" {} ", segment.label),
            Style::new()
                .fg(THEME_TEXT)
                .bg(THEME_MARKED)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

fn breadcrumb_path_at(path: &str, header: Rect, column: u16, row: u16) -> Option<String> {
    let inner = inner_rect(header);
    if row != inner.y || column < inner.x || column >= inner.right() {
        return None;
    }

    let mut cursor = inner
        .x
        .saturating_add(text_width_u16(HEADER_BRAND))
        .saturating_add(text_width_u16(HEADER_GAP));
    for (index, segment) in breadcrumb_segments(path).into_iter().enumerate() {
        if index > 0 {
            cursor = cursor.saturating_add(text_width_u16(BREADCRUMB_SEPARATOR));
        }
        let label_width = text_width_u16(segment.label.as_str()).saturating_add(2);
        let end = cursor.saturating_add(label_width);
        if column >= cursor && column < end {
            return Some(segment.target);
        }
        cursor = end;
    }
    None
}

fn text_width_u16(value: &str) -> u16 {
    UnicodeWidthStr::width(value).min(usize::from(u16::MAX)) as u16
}

fn header_widget(app: &TuiApp) -> Paragraph<'_> {
    let mut spans = vec![
        Span::styled(
            HEADER_BRAND,
            Style::new()
                .fg(THEME_ACCENT)
                .bg(THEME_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(HEADER_GAP, surface_style()),
    ];
    spans.extend(breadcrumb_spans(app.path()));
    spans.extend([
        Span::styled(HEADER_GAP, surface_style()),
        Span::styled(
            app.request_state.user_label(),
            Style::new()
                .fg(request_state_color(app.request_state))
                .bg(THEME_BG)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    Paragraph::new(Line::from(spans))
        .block(themed_block("Session", false))
        .style(surface_style())
}

fn render_files_pane(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    frame.render_widget(files_widget(app, area), area);

    let rows = area.height.saturating_sub(2) as usize;
    let total = app.filtered_entry_count();
    if total <= rows || rows == 0 {
        return;
    }

    let range = app.visible_entry_range(rows);
    let mut state = ScrollbarState::new(total)
        .position(range.start)
        .viewport_content_length(rows);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(Style::new().fg(THEME_ACCENT).bg(THEME_PANEL))
            .track_style(Style::new().fg(THEME_BORDER).bg(THEME_PANEL)),
        inner_rect(area),
        &mut state,
    );
}

fn files_widget(app: &TuiApp, area: Rect) -> List<'_> {
    let rows = area.height.saturating_sub(2) as usize;
    let visible_indices = app.filtered_indices();
    let range = app.visible_entry_range(rows);
    let inner_width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem<'_>> = if range.is_empty() {
        vec![ListItem::new(
            Line::raw(files_empty_message(app)).style(meta_value_style()),
        )]
    } else {
        visible_indices[range.clone()]
            .iter()
            .map(|index| {
                let entry = &app.entries()[*index];
                let selected = app.selected_index() == Some(*index);
                let marked = app.path_is_marked(entry.path.as_str());
                let cursor_marker = if selected { ">" } else { " " };
                let mark_marker = if marked { "[x]" } else { "[ ]" };
                let suffix = if entry.is_dir { "/" } else { "" };
                let size = file_size_label(entry);
                let row_bg = if selected {
                    THEME_SELECTED
                } else if marked {
                    THEME_MARKED
                } else {
                    THEME_PANEL
                };
                let name_width = inner_width
                    .saturating_sub(UnicodeWidthStr::width(cursor_marker))
                    .saturating_sub(1)
                    .saturating_sub(UnicodeWidthStr::width(mark_marker))
                    .saturating_sub(2)
                    .saturating_sub(UnicodeWidthStr::width(size.as_str()))
                    .saturating_sub(2)
                    .max(4);
                let name_value = format!("{}{}", entry.server_filename, suffix);
                let name = if selected {
                    marquee_cells(name_value.as_str(), name_width, app.marquee_tick)
                } else {
                    truncate_cells(name_value.as_str(), name_width)
                };
                let base_style = if selected {
                    Style::new()
                        .fg(THEME_TEXT)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(THEME_TEXT).bg(row_bg)
                };
                let name_style = if selected {
                    base_style
                } else if entry.is_dir {
                    Style::new()
                        .fg(THEME_DIRECTORY)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(THEME_TEXT).bg(row_bg)
                };
                let cursor_style = if selected {
                    Style::new()
                        .fg(THEME_ACCENT)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(THEME_DIM).bg(row_bg)
                };
                let mark_style = if marked {
                    Style::new()
                        .fg(THEME_WARNING)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(THEME_DIM).bg(row_bg)
                };
                let size_style = if selected {
                    Style::new().fg(THEME_TEXT).bg(row_bg)
                } else {
                    Style::new().fg(THEME_DIM).bg(row_bg)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(cursor_marker, cursor_style),
                    Span::styled(" ", base_style),
                    Span::styled(mark_marker, mark_style),
                    Span::styled(" ", base_style),
                    Span::styled(name, name_style),
                    Span::styled("  ", base_style),
                    Span::styled(size, size_style),
                ]))
                .style(base_style)
            })
            .collect()
    };

    let total = visible_indices.len();
    let title = if total == 0 {
        if app.filter_input().is_empty() {
            "Files 0 items".to_string()
        } else {
            format!("Files 0/{} items", app.entries().len())
        }
    } else if app.filter_input().is_empty() {
        format!("Files {}-{}/{}", range.start + 1, range.end, total)
    } else {
        format!(
            "Files {}-{}/{} of {}",
            range.start + 1,
            range.end,
            total,
            app.entries().len()
        )
    };
    List::new(items)
        .block(themed_block(title, app.focus() == PanelFocus::Files))
        .style(surface_style())
}

fn preview_widget(app: &TuiApp, preview: Option<PreviewBudget>, area: Rect) -> Paragraph<'_> {
    let inner = inner_rect(area);
    let inner_width = usize::from(inner.width);
    let selected = app.selected_entry();
    let title = if app.preview_title.is_empty() {
        selected
            .map(|entry| entry.server_filename.as_str())
            .or_else(|| app.notice())
            .unwrap_or("metadata")
    } else {
        app.preview_title.as_str()
    };
    let budget = preview.unwrap_or(app.preview_budget);

    let mut lines = selected
        .map(|entry| {
            preview_detail_lines(
                entry,
                app.cache_source(),
                budget,
                inner_width,
                app.marquee_tick,
            )
        })
        .unwrap_or_else(|| {
            let name_width = detail_value_width(inner_width, "Name:");
            vec![
                detail_line("Name:", marquee_cells(title, name_width, app.marquee_tick)),
                detail_line("Kind:", "none".to_string()),
                detail_line("Cache:", app.cache_source().user_label().to_string()),
            ]
        });
    lines.push(Line::raw("").style(surface_style()));
    if budget.kind == PreviewKind::Video && !app.preview_lines.is_empty() {
        push_plain_lines(&mut lines, &app.preview_lines, "", content_style());
        lines.push(Line::raw("").style(surface_style()));
        lines.push(detail_line(
            "Buffer:",
            format!(
                "{} / {} · {}",
                bytes_label(budget.fetched_bytes),
                bytes_label(budget.memory_limit_bytes),
                cache_storage_label(budget.disk_backed)
            ),
        ));
    } else if budget.kind == PreviewKind::Text && !app.preview_lines.is_empty() {
        lines.truncate(4);
        lines.push(detail_line(
            "Preview:",
            format!(
                "text · {} fetched · {}",
                bytes_label(budget.fetched_bytes),
                cache_storage_label(budget.disk_backed)
            ),
        ));
        lines.push(Line::raw("").style(surface_style()));
        push_plain_lines(&mut lines, &app.preview_lines, "", content_style());
    } else {
        lines.push(detail_line("Preview:", budget.kind.label().to_string()));
        lines.push(Line::raw("").style(surface_style()));
        push_plain_lines(&mut lines, &app.preview_lines, "", content_style());
    }

    let scroll = app
        .preview_scroll_offset
        .min(lines.len().saturating_sub(usize::from(inner.height)))
        .min(usize::from(u16::MAX)) as u16;
    Paragraph::new(lines)
        .block(themed_block("Preview", app.focus() == PanelFocus::Preview))
        .style(surface_style())
        .scroll((scroll, 0))
        .wrap(Wrap { trim: true })
}

fn preview_detail_lines(
    entry: &FileEntry,
    source: CacheSource,
    budget: PreviewBudget,
    inner_width: usize,
    marquee_tick: u64,
) -> Vec<Line<'static>> {
    let item_type = if entry.is_dir { "directory" } else { "file" };
    let action = if entry.is_dir {
        "l enter directory"
    } else if budget.kind == PreviewKind::Video {
        "p preview video"
    } else {
        "p open local copy (<100MiB)"
    };
    let name_width = detail_value_width(inner_width, "Name:");
    let path_width = detail_value_width(inner_width, "Path:");
    vec![
        detail_line(
            "Name:",
            marquee_cells(entry.server_filename.as_str(), name_width, marquee_tick),
        ),
        detail_line("Kind:", format!("{item_type} · {}", budget.kind.label())),
        detail_line("Size:", file_size_label(entry)),
        detail_line("Modified:", modified_time_label(entry.mtime)),
        detail_line("Cache:", source.user_label().to_string()),
        detail_line("ID:", entry.fs_id.to_string()),
        detail_line(
            "Path:",
            marquee_cells(entry.path.as_str(), path_width, marquee_tick),
        ),
        detail_line("Action:", action.to_string()),
    ]
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

fn push_limited_lines(
    output: &mut Vec<Line<'static>>,
    source: &[String],
    limit: usize,
    prefix: &str,
    style: Style,
) {
    if limit == 0 {
        return;
    }
    let shown = if source.len() > limit {
        limit.saturating_sub(1)
    } else {
        limit
    };
    for line in source.iter().take(shown) {
        output.push(Line::raw(format!("{prefix}{line}")).style(style));
    }
    if source.len() > limit {
        output.push(Line::raw(format!("+{} more", source.len() - shown)).style(meta_value_style()));
    }
}

fn push_plain_lines(
    output: &mut Vec<Line<'static>>,
    source: &[String],
    prefix: &str,
    style: Style,
) {
    output.extend(
        source
            .iter()
            .map(|line| Line::raw(format!("{prefix}{line}")).style(style)),
    );
}

fn tasks_widget(app: &TuiApp) -> Paragraph<'_> {
    let mut lines = Vec::new();
    if let Some(line) = task_status_line(app) {
        lines.push(line);
    }
    push_limited_lines(
        &mut lines,
        &app.preload_lines,
        2,
        "preload ",
        preload_style(),
    );
    push_limited_lines(&mut lines, &app.task_lines, 3, "", content_style());
    if let Some(notice) = app.notice() {
        lines.push(Line::raw(notice.to_string()).style(notice_style()));
    }
    if lines.is_empty() {
        lines.push(Line::raw("No active tasks").style(meta_value_style()));
    }
    Paragraph::new(lines)
        .block(themed_block("Tasks", false))
        .style(surface_style())
}

fn confirm_widget(app: &TuiApp) -> Paragraph<'_> {
    let mut lines = Vec::new();
    let body_lines = confirm_body_lines(app);
    for (index, line) in body_lines.iter().take(9).enumerate() {
        let style = if index == 0 {
            confirm_style()
        } else {
            content_style()
        };
        lines.push(Line::raw(line.to_string()).style(style));
    }
    if body_lines.len() > 9 {
        lines.push(Line::raw(format!("+{} more", body_lines.len() - 9)).style(meta_value_style()));
    }
    lines.push(Line::raw("").style(surface_style()));
    lines.push(Line::from(vec![
        Span::styled("[y] confirm", confirm_style()),
        Span::styled("    ", surface_style()),
        Span::styled("[n] cancel", warning_style()),
    ]));

    Paragraph::new(lines)
        .block(
            Block::bordered()
                .style(surface_style())
                .border_style(Style::new().fg(THEME_ERROR).bg(THEME_BG))
                .title(Line::from(Span::styled(
                    confirm_title(app),
                    Style::new()
                        .fg(THEME_ERROR)
                        .bg(THEME_BG)
                        .add_modifier(Modifier::BOLD),
                ))),
        )
        .style(surface_style())
        .wrap(Wrap { trim: true })
}

fn confirm_title(app: &TuiApp) -> &'static str {
    match app.confirm_kind {
        Some(ConfirmKind::Quit) => "Quit?",
        Some(ConfirmKind::Operation) | None => "Confirm",
    }
}

fn confirm_body_lines(app: &TuiApp) -> Vec<String> {
    let mut lines = app.confirm_lines.clone();
    if lines
        .last()
        .is_some_and(|line| line.contains("press y") || line.contains("confirm"))
    {
        lines.pop();
    }
    lines
}

fn status_widget(app: &TuiApp) -> Paragraph<'_> {
    if app.mode == AppMode::Command {
        return input_status_widget(":", app.command_input());
    }
    if app.mode == AppMode::Filter {
        return input_status_widget("/", app.filter_input());
    }

    let mut spans = vec![
        Span::styled(
            app.mode.label(),
            Style::new()
                .fg(THEME_BG)
                .bg(THEME_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG)),
        Span::styled(
            app.position_label(),
            Style::new().fg(THEME_TEXT).bg(THEME_BG),
        ),
        Span::styled("  ", Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG)),
        Span::styled(
            format!("{} selected", app.selected_paths.len()),
            Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG),
        ),
        Span::styled("  ", Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG)),
        Span::styled(
            app.path().to_string(),
            Style::new().fg(THEME_TEXT).bg(THEME_BG),
        ),
    ];
    if let Some(hint) = app.status_hint() {
        spans.push(Span::styled(
            "  ",
            Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG),
        ));
        spans.push(Span::styled(
            hint,
            Style::new()
                .fg(request_state_color(app.request_state))
                .bg(THEME_BG),
        ));
    }
    Paragraph::new(Line::from(spans)).style(Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG))
}

fn input_status_widget(prompt: &str, input: &str) -> Paragraph<'static> {
    Paragraph::new(Line::raw(format!("{prompt}{input}|"))).style(
        Style::new()
            .fg(THEME_ACCENT)
            .bg(THEME_BG)
            .add_modifier(Modifier::BOLD),
    )
}

fn request_state_color(state: DirectoryRequestState) -> Color {
    match state {
        DirectoryRequestState::Idle => THEME_SUCCESS,
        DirectoryRequestState::Loading | DirectoryRequestState::Refreshing => THEME_WARNING,
        DirectoryRequestState::Failed => THEME_ERROR,
    }
}

fn file_size_label(entry: &FileEntry) -> String {
    if entry.is_dir {
        return "<DIR>".to_string();
    }

    let bytes = u64::try_from(entry.size).unwrap_or(0);
    bytes_label(bytes)
}

fn bytes_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len().saturating_sub(1) {
        value /= 1024.0;
        unit_index = unit_index.saturating_add(1);
    }
    format!("{value:.1} {}", UNITS[unit_index])
}

fn modified_time_label(timestamp: i64) -> String {
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp(timestamp) else {
        return "unknown".to_string();
    };
    datetime
        .format(format_description!("[year]-[month]-[day] [hour]:[minute]"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn cache_storage_label(disk_backed: bool) -> &'static str {
    if disk_backed {
        "disk backed"
    } else {
        "memory only"
    }
}

fn files_empty_message(app: &TuiApp) -> String {
    if !app.filter_input().is_empty() {
        return format!("No matches for \"{}\"", app.filter_input());
    }
    match app.request_state {
        DirectoryRequestState::Loading => "Loading...".to_string(),
        DirectoryRequestState::Refreshing => "Refreshing cached entries...".to_string(),
        DirectoryRequestState::Failed => app
            .notice()
            .map(|notice| format!("Error: {notice}"))
            .unwrap_or_else(|| "Could not load directory".to_string()),
        DirectoryRequestState::Idle => "Empty directory".to_string(),
    }
}

fn task_status_line(app: &TuiApp) -> Option<Line<'static>> {
    match app.request_state {
        DirectoryRequestState::Loading => Some(
            Line::raw(format!("Loading {}", app.path())).style(
                Style::new()
                    .fg(request_state_color(app.request_state))
                    .bg(THEME_PANEL),
            ),
        ),
        DirectoryRequestState::Refreshing => Some(
            Line::raw(format!("Refreshing {} from cache", app.path())).style(
                Style::new()
                    .fg(request_state_color(app.request_state))
                    .bg(THEME_PANEL),
            ),
        ),
        DirectoryRequestState::Failed => app.notice().map(|notice| {
            Line::raw(format!("Error: {notice}")).style(
                Style::new()
                    .fg(request_state_color(app.request_state))
                    .bg(THEME_PANEL),
            )
        }),
        DirectoryRequestState::Idle => (app.background_tasks > 0).then(|| {
            Line::raw(format!(
                "{} {} running",
                app.background_tasks,
                pluralize(app.background_tasks, "task", "tasks")
            ))
            .style(meta_value_style())
        }),
    }
}

fn pluralize(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 {
        singular
    } else {
        plural
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

fn marquee_cells(value: &str, max_width: usize, tick: u64) -> String {
    let width = UnicodeWidthStr::width(value);
    if width <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }

    let offset = marquee_offset(width, max_width, tick);
    cell_window(value, offset, max_width)
}

fn marquee_offset(width: usize, max_width: usize, tick: u64) -> usize {
    let overflow = width.saturating_sub(max_width);
    if overflow == 0 {
        return 0;
    }

    let speed = 1 + (overflow / 32).min(4);
    let move_ticks = overflow.div_ceil(speed).max(1) as u64;
    let pause = MARQUEE_EDGE_PAUSE_TICKS;
    let cycle = pause
        .saturating_add(move_ticks)
        .saturating_add(pause)
        .saturating_add(move_ticks);
    let mut phase = tick % cycle.max(1);
    if phase < pause {
        return 0;
    }
    phase = phase.saturating_sub(pause);
    if phase < move_ticks {
        return (phase as usize).saturating_mul(speed).min(overflow);
    }
    phase = phase.saturating_sub(move_ticks);
    if phase < pause {
        return overflow;
    }
    phase = phase.saturating_sub(pause);
    overflow.saturating_sub((phase as usize).saturating_mul(speed).min(overflow))
}

fn cell_window(value: &str, start: usize, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut output = String::new();
    let mut cursor: usize = 0;
    let mut used: usize = 0;
    for ch in value.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        let next_cursor = cursor.saturating_add(width);
        if width > 0 && next_cursor <= start {
            cursor = next_cursor;
            continue;
        }
        if used.saturating_add(width) > max_width {
            break;
        }
        output.push(ch);
        used = used.saturating_add(width);
        cursor = next_cursor;
    }

    output
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewKind {
    Text,
    Image,
    Pdf,
    Archive,
    Audio,
    Document,
    Ebook,
    Font,
    Directory,
    Metadata,
    Video,
}

impl PreviewKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Pdf => "pdf",
            Self::Archive => "archive",
            Self::Audio => "audio",
            Self::Document => "document",
            Self::Ebook => "ebook",
            Self::Font => "font",
            Self::Directory => "directory",
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
