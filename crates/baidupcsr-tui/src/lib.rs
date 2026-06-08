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
        Block, Cell, Clear, Gauge, HighlightSpacing, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableState, Wrap,
    },
    Frame,
};
use std::{cmp::Ordering, collections::BTreeMap, io, ops::Range, time::Duration};
use time::{macros::format_description, OffsetDateTime};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
enum AppMode {
    Normal,
    Filter,
    Search,
    Rename,
    Sort,
    Command,
    Confirm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PanelFocus {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollbarDragTarget {
    Files,
    Preview,
}

impl AppMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Filter => "FILTER",
            Self::Search => "SEARCH",
            Self::Rename => "RENAME",
            Self::Sort => "SORT",
            Self::Command => "COMMAND",
            Self::Confirm => "CONFIRM",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SortKey {
    Name,
    Modified,
    Size,
    Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SortMode {
    key: SortKey,
    direction: SortDirection,
}

impl SortKey {
    const fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Modified => "modified",
            Self::Size => "size",
            Self::Type => "type",
        }
    }
}

impl SortDirection {
    const fn marker(self) -> &'static str {
        match self {
            Self::Ascending => "asc",
            Self::Descending => "desc",
        }
    }
}

impl SortMode {
    const fn default() -> Self {
        Self {
            key: SortKey::Name,
            direction: SortDirection::Ascending,
        }
    }

    fn label(self) -> String {
        format!("sort {} {}", self.key.label(), self.direction.marker())
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
    Filter,
    Search,
    Command,
    Refresh,
    Rename,
    Delete,
    Sort,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseInputKind {
    LeftClick,
    DoubleClick,
    LeftDrag,
    LeftRelease,
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
    SearchSubmitted(String),
    RenameSubmitted(String),
    PathRequested(String),
    ConfirmAccepted,
    ConfirmRejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskPanelItem {
    pub id: u64,
    pub kind: String,
    pub state: String,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub speed_bytes_per_sec: u64,
    pub label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectoryRequestState {
    Idle,
    Loading,
    Refreshing,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SearchResultsState {
    root_path: String,
    keyword: String,
    restore_entries: Vec<FileEntry>,
    restore_selection_path: Option<String>,
    scanned_dirs: usize,
    scanned_entries: usize,
    matches: usize,
    running: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FileListMode {
    Directory,
    SearchResults(SearchResultsState),
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
    Disk,
    Network,
}

impl CacheSource {
    const fn user_label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Disk => "disk",
            Self::Network => "network",
        }
    }
}

const fn key_action_for(event: KeyEvent) -> Option<KeyAction> {
    match event {
        KeyEvent::Char('j') | KeyEvent::Down => Some(KeyAction::Down),
        KeyEvent::Char('k') | KeyEvent::Up => Some(KeyAction::Up),
        KeyEvent::Char('h') | KeyEvent::Left => Some(KeyAction::Back),
        KeyEvent::Char('l') | KeyEvent::Right => Some(KeyAction::Enter),
        KeyEvent::PageUp => Some(KeyAction::PageUp),
        KeyEvent::PageDown => Some(KeyAction::PageDown),
        KeyEvent::Home => Some(KeyAction::Top),
        KeyEvent::End | KeyEvent::Char('G') => Some(KeyAction::Bottom),
        KeyEvent::Ctrl('u' | 'U') => Some(KeyAction::HalfPageUp),
        KeyEvent::Ctrl('d' | 'D') => Some(KeyAction::HalfPageDown),
        KeyEvent::Char('/') | KeyEvent::Char('f') => Some(KeyAction::Filter),
        KeyEvent::Char('s') => Some(KeyAction::Search),
        KeyEvent::Char(':') => Some(KeyAction::Command),
        KeyEvent::Char('r') => Some(KeyAction::Rename),
        KeyEvent::Char('u') => Some(KeyAction::Refresh),
        KeyEvent::Char('p') => Some(KeyAction::Preview),
        KeyEvent::Char('d') => Some(KeyAction::Download),
        KeyEvent::Char('x') => Some(KeyAction::Delete),
        KeyEvent::Char('c') => Some(KeyAction::CancelTask),
        KeyEvent::Char(',') => Some(KeyAction::Sort),
        KeyEvent::Char(' ') => Some(KeyAction::ToggleSelect),
        KeyEvent::Char('q') => Some(KeyAction::Quit),
        KeyEvent::Esc
        | KeyEvent::Enter
        | KeyEvent::Backspace
        | KeyEvent::Tab
        | KeyEvent::Ctrl(_)
        | KeyEvent::Char(_) => None,
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
    task_items: Vec<TaskPanelItem>,
    task_speed_history: BTreeMap<u64, Vec<u64>>,
    preload_lines: Vec<String>,
    command_input: String,
    filter_input: String,
    search_input: String,
    rename_input: String,
    sort_mode: SortMode,
    file_list_mode: FileListMode,
    selected_paths: Vec<String>,
    confirm_lines: Vec<String>,
    confirm_kind: Option<ConfirmKind>,
    help_visible: bool,
    pending_g: bool,
    scroll_offset: usize,
    preview_scroll_offset: usize,
    marquee_tick: u64,
    last_file_click: Option<FileClick>,
    scrollbar_drag: Option<ScrollbarDragTarget>,
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
            task_items: Vec::new(),
            task_speed_history: BTreeMap::new(),
            preload_lines: Vec::new(),
            command_input: String::new(),
            filter_input: String::new(),
            search_input: String::new(),
            rename_input: String::new(),
            sort_mode: SortMode::default(),
            file_list_mode: FileListMode::Directory,
            selected_paths: Vec::new(),
            confirm_lines: Vec::new(),
            confirm_kind: None,
            help_visible: false,
            pending_g: false,
            scroll_offset: 0,
            preview_scroll_offset: 0,
            marquee_tick: 0,
            last_file_click: None,
            scrollbar_drag: None,
            focus: PanelFocus::Files,
        }
    }

    pub fn set_entries(&mut self, entries: Vec<FileEntry>) {
        self.set_entries_with_selection(entries, None);
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
        self.file_list_mode = FileListMode::Directory;
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
        self.file_list_mode = FileListMode::Directory;
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
        self.file_list_mode = FileListMode::Directory;
        self.cache_age_seconds = cache_age_seconds;
        self.background_tasks = 0;
        self.request_state = DirectoryRequestState::Failed;
        self.set_entries_with_selection(cached_entries, preferred_selection_path);
        self.notice = Some(message.into());
    }

    pub fn set_background_tasks(&mut self, count: usize) {
        self.background_tasks = count;
    }

    pub fn set_task_items(&mut self, items: Vec<TaskPanelItem>) {
        self.task_speed_history
            .retain(|id, _| items.iter().any(|item| item.id == *id));
        for item in &items {
            let history = self.task_speed_history.entry(item.id).or_default();
            if item.speed_bytes_per_sec > 0 {
                history.push(item.speed_bytes_per_sec);
                if history.len() > 20 {
                    history.drain(0..history.len() - 20);
                }
            }
        }
        self.task_items = items;
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

    pub fn sort_mode_label(&self) -> String {
        self.sort_mode.label()
    }

    pub fn begin_search_results(&mut self, root_path: &str, keyword: &str) {
        let restore_selection_path = self.selected_entry().map(|entry| entry.path.clone());
        let restore_entries = self.entries.clone();
        let root_path = normalize_path(root_path);
        let keyword = keyword.to_string();
        self.path = root_path.clone();
        self.entries.clear();
        self.selected_index = None;
        self.scroll_offset = 0;
        self.filter_input.clear();
        self.search_input.clear();
        self.selected_paths.clear();
        self.last_file_click = None;
        self.background_tasks = 1;
        self.request_state = DirectoryRequestState::Idle;
        self.notice = Some(format!("searching \"{keyword}\" in {root_path}"));
        self.file_list_mode = FileListMode::SearchResults(SearchResultsState {
            root_path,
            keyword,
            restore_entries,
            restore_selection_path,
            scanned_dirs: 0,
            scanned_entries: 0,
            matches: 0,
            running: true,
        });
        self.begin_preview("search", "searching remote files");
    }

    pub fn append_search_results(
        &mut self,
        entries: Vec<FileEntry>,
        scanned_dirs: usize,
        scanned_entries: usize,
    ) {
        let Some(state) = self.search_results_state_mut() else {
            return;
        };
        state.scanned_dirs = scanned_dirs;
        state.scanned_entries = scanned_entries;
        state.matches = state.matches.saturating_add(entries.len());
        self.entries.extend(entries);
        self.sort_entries_preserving_selection();
        if self.selected_index.is_none() && !self.entries.is_empty() {
            self.select_index(Some(0));
        }
        self.sync_selection_to_filter();
        self.update_search_notice();
    }

    pub fn finish_search_success(&mut self, scanned_dirs: usize, scanned_entries: usize) {
        let matches = self.entries.len();
        let Some(state) = self.search_results_state_mut() else {
            return;
        };
        state.scanned_dirs = scanned_dirs;
        state.scanned_entries = scanned_entries;
        state.matches = matches;
        state.running = false;
        self.background_tasks = 0;
        self.update_search_notice();
    }

    pub fn finish_search_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        let Some(state) = self.take_search_results_state() else {
            self.notice = Some(format!("search_failed: {message}"));
            self.background_tasks = 0;
            return;
        };
        let root_path = state.root_path;
        let restore_selection_path = state.restore_selection_path;
        self.path = root_path;
        self.background_tasks = 0;
        self.request_state = DirectoryRequestState::Idle;
        self.notice = Some(format!("search_failed: {message}"));
        self.set_entries_with_selection(state.restore_entries, restore_selection_path.as_deref());
    }

    pub fn exit_search_results(&mut self) -> Option<String> {
        let state = self.take_search_results_state()?;
        let root_path = state.root_path;
        let restore_selection_path = state.restore_selection_path;
        self.path = root_path.clone();
        self.background_tasks = 0;
        self.request_state = DirectoryRequestState::Idle;
        self.notice = Some(format!("returned to {}", root_path));
        self.set_entries_with_selection(state.restore_entries, restore_selection_path.as_deref());
        Some(root_path)
    }

    pub fn is_search_results(&self) -> bool {
        matches!(self.file_list_mode, FileListMode::SearchResults(_))
    }

    pub fn search_origin_path(&self) -> Option<&str> {
        match &self.file_list_mode {
            FileListMode::Directory => None,
            FileListMode::SearchResults(state) => Some(state.root_path.as_str()),
        }
    }

    fn begin_rename_input(&mut self) {
        let Some(name) = self
            .selected_entry()
            .map(|entry| entry.server_filename.clone())
        else {
            self.notice = Some("rename_error: no selected entry".to_string());
            return;
        };
        self.mode = AppMode::Rename;
        self.rename_input = name;
    }

    fn begin_sort_menu(&mut self) {
        self.mode = AppMode::Sort;
    }

    fn choose_sort_key(&mut self, key: SortKey) {
        if self.sort_mode.key == key {
            self.sort_mode.direction = match self.sort_mode.direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort_mode = SortMode {
                key,
                direction: default_sort_direction(key),
            };
        }
        self.mode = AppMode::Normal;
        self.sort_entries_preserving_selection();
        self.sync_selection_to_filter();
        self.notice = Some(self.sort_mode.label());
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

    pub fn handle_mouse_event(&mut self, input: MouseInput) -> Option<TuiRuntimeEvent> {
        if input.kind == MouseInputKind::LeftRelease {
            self.scrollbar_drag = None;
            return None;
        }

        if self.help_visible {
            if matches!(
                input.kind,
                MouseInputKind::LeftClick | MouseInputKind::DoubleClick | MouseInputKind::LeftDrag
            ) {
                self.help_visible = false;
            }
            return None;
        }

        let full = full_layout(input.area, !self.task_items.is_empty());
        let layout = browser_layout(full.body);
        let files_inner = inner_rect(layout.files);
        let preview_inner = inner_rect(layout.preview);
        let files_scrollbar = vertical_scrollbar_rect(files_inner);
        let preview_scrollbar = vertical_scrollbar_rect(preview_inner);

        match input.kind {
            MouseInputKind::ScrollUp => {
                self.scrollbar_drag = None;
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
                self.scrollbar_drag = None;
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
            MouseInputKind::LeftDrag => match self.scrollbar_drag {
                Some(ScrollbarDragTarget::Files) => self.handle_files_scrollbar_pointer(
                    input.row,
                    files_scrollbar?,
                    usize::from(files_inner.height),
                ),
                Some(ScrollbarDragTarget::Preview) => {
                    self.handle_preview_scrollbar_pointer(
                        input.row,
                        preview_scrollbar?,
                        usize::from(preview_inner.height),
                    );
                    None
                }
                None => {
                    if let Some(scrollbar) = files_scrollbar
                        .filter(|scrollbar| rect_contains(*scrollbar, input.column, input.row))
                    {
                        self.scrollbar_drag = Some(ScrollbarDragTarget::Files);
                        return self.handle_files_scrollbar_pointer(
                            input.row,
                            scrollbar,
                            usize::from(files_inner.height),
                        );
                    }
                    if let Some(scrollbar) = preview_scrollbar
                        .filter(|scrollbar| rect_contains(*scrollbar, input.column, input.row))
                    {
                        self.scrollbar_drag = Some(ScrollbarDragTarget::Preview);
                        self.handle_preview_scrollbar_pointer(
                            input.row,
                            scrollbar,
                            usize::from(preview_inner.height),
                        );
                    }
                    None
                }
            },
            MouseInputKind::LeftClick | MouseInputKind::DoubleClick => {
                if let Some(scrollbar) = files_scrollbar
                    .filter(|scrollbar| rect_contains(*scrollbar, input.column, input.row))
                {
                    self.scrollbar_drag = Some(ScrollbarDragTarget::Files);
                    return self.handle_files_scrollbar_pointer(
                        input.row,
                        scrollbar,
                        usize::from(files_inner.height),
                    );
                }
                if let Some(scrollbar) = preview_scrollbar
                    .filter(|scrollbar| rect_contains(*scrollbar, input.column, input.row))
                {
                    self.scrollbar_drag = Some(ScrollbarDragTarget::Preview);
                    self.handle_preview_scrollbar_pointer(
                        input.row,
                        scrollbar,
                        usize::from(preview_inner.height),
                    );
                    return None;
                }

                self.scrollbar_drag = None;

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
            MouseInputKind::LeftRelease => None,
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
            AppMode::Search => self.handle_search_key(event),
            AppMode::Rename => self.handle_rename_key(event),
            AppMode::Sort => self.handle_sort_key(event),
            AppMode::Confirm => self.handle_confirm_key(event),
        }
    }

    pub const fn selected_index(&self) -> Option<usize> {
        self.selected_index
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

    fn filtered_entry_count(&self) -> usize {
        self.filtered_indices().len()
    }

    pub fn selected_paths(&self) -> &[String] {
        &self.selected_paths
    }

    fn search_results_state(&self) -> Option<&SearchResultsState> {
        match &self.file_list_mode {
            FileListMode::Directory => None,
            FileListMode::SearchResults(state) => Some(state),
        }
    }

    fn search_results_state_mut(&mut self) -> Option<&mut SearchResultsState> {
        match &mut self.file_list_mode {
            FileListMode::Directory => None,
            FileListMode::SearchResults(state) => Some(state),
        }
    }

    fn take_search_results_state(&mut self) -> Option<SearchResultsState> {
        match std::mem::replace(&mut self.file_list_mode, FileListMode::Directory) {
            FileListMode::Directory => None,
            FileListMode::SearchResults(state) => Some(state),
        }
    }

    fn update_search_notice(&mut self) {
        let Some(state) = self.search_results_state() else {
            return;
        };
        let status = if state.running {
            "searching"
        } else {
            "search_complete"
        };
        self.notice = Some(format!(
            "{status}: {} matches for \"{}\" in {} ({} dirs, {} entries)",
            state.matches,
            state.keyword,
            state.root_path,
            state.scanned_dirs,
            state.scanned_entries
        ));
    }

    fn visible_entry_range(&self, rows: usize) -> Range<usize> {
        let indices = self.filtered_indices();
        visible_range_from_offset(indices.len(), self.scroll_offset, rows)
    }

    pub fn parent_path(path: &str) -> Option<String> {
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() || trimmed == "/" {
            return None;
        }

        match trimmed.rfind('/') {
            Some(0) | None => Some("/".to_string()),
            Some(index) => Some(trimmed[..index].to_string()),
        }
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

    fn scroll_files_to_offset(&mut self, offset: usize, rows: usize) -> bool {
        let old_offset = self.scroll_offset;
        let old_selection = self.selected_index;
        let indices = self.filtered_indices();
        if indices.is_empty() {
            self.scroll_offset = 0;
            self.select_index(None);
            return old_offset != self.scroll_offset || old_selection != self.selected_index;
        }
        if rows == 0 {
            return false;
        }

        let rows = rows.min(indices.len());
        let start = offset.min(indices.len().saturating_sub(rows));
        let end = start.saturating_add(rows);
        self.scroll_offset = start;

        let target_position = match self.selected_visible_position(&indices) {
            Some(position) if position >= start && position < end => position,
            Some(position) if position < start => start,
            Some(_) => end.saturating_sub(1),
            None => start,
        };
        self.select_index(indices.get(target_position).copied());

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

    fn scroll_preview_to_offset(&mut self, offset: usize, rows: usize) -> bool {
        let old_offset = self.preview_scroll_offset;
        let total = self.preview_render_line_count();
        if total == 0 || rows == 0 {
            self.preview_scroll_offset = 0;
            return old_offset != self.preview_scroll_offset;
        }

        let last_start = total.saturating_sub(rows.min(total));
        self.preview_scroll_offset = offset.min(last_start);
        old_offset != self.preview_scroll_offset
    }

    fn handle_files_scrollbar_pointer(
        &mut self,
        row: u16,
        scrollbar: Rect,
        rows: usize,
    ) -> Option<TuiRuntimeEvent> {
        let offset = scrollbar_offset_at_row(self.filtered_entry_count(), rows, scrollbar, row)?;
        self.focus = PanelFocus::Files;
        self.scroll_files_to_offset(offset, rows)
            .then_some(TuiRuntimeEvent::Action(KeyAction::Select))
    }

    fn handle_preview_scrollbar_pointer(&mut self, row: u16, scrollbar: Rect, rows: usize) {
        let Some(offset) =
            scrollbar_offset_at_row(self.preview_render_line_count(), rows, scrollbar, row)
        else {
            return;
        };
        self.focus = PanelFocus::Preview;
        self.scroll_preview_to_offset(offset, rows);
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
            if self.is_search_results() {
                return Some(TuiRuntimeEvent::Action(KeyAction::Back));
            }
            return None;
        }

        let action = key_action_for(event)?;
        match action {
            KeyAction::Down => {
                self.move_visible_by(1);
                return Some(TuiRuntimeEvent::Action(KeyAction::Down));
            }
            KeyAction::Up => {
                self.move_visible_by(-1);
                return Some(TuiRuntimeEvent::Action(KeyAction::Up));
            }
            KeyAction::PageDown => {
                self.move_visible_by(PAGE_STEP as isize);
                return Some(TuiRuntimeEvent::Action(KeyAction::PageDown));
            }
            KeyAction::PageUp => {
                self.move_visible_by(-(PAGE_STEP as isize));
                return Some(TuiRuntimeEvent::Action(KeyAction::PageUp));
            }
            KeyAction::HalfPageDown => {
                self.move_visible_by(HALF_PAGE_STEP as isize);
                return Some(TuiRuntimeEvent::Action(KeyAction::HalfPageDown));
            }
            KeyAction::HalfPageUp => {
                self.move_visible_by(-(HALF_PAGE_STEP as isize));
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
            KeyAction::Filter => {
                self.mode = AppMode::Filter;
                self.filter_input.clear();
                self.sync_selection_to_filter();
            }
            KeyAction::Search => {
                self.mode = AppMode::Search;
                self.search_input.clear();
            }
            KeyAction::Rename => self.begin_rename_input(),
            KeyAction::Sort => self.begin_sort_menu(),
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
            | KeyAction::Delete
            | KeyAction::Preview
            | KeyAction::Download
            | KeyAction::CancelTask
            | KeyAction::Select => {
                return Some(TuiRuntimeEvent::Action(action));
            }
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

    fn handle_search_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Esc => {
                self.mode = AppMode::Normal;
                self.search_input.clear();
                None
            }
            KeyEvent::Backspace => {
                self.search_input.pop();
                None
            }
            KeyEvent::Enter => {
                let input = self.search_input.trim().to_string();
                self.search_input.clear();
                self.mode = AppMode::Normal;
                if input.is_empty() {
                    None
                } else {
                    Some(TuiRuntimeEvent::SearchSubmitted(input))
                }
            }
            KeyEvent::Char(ch) => {
                self.search_input.push(ch);
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

    fn handle_rename_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Esc => {
                self.mode = AppMode::Normal;
                self.rename_input.clear();
                None
            }
            KeyEvent::Backspace => {
                self.rename_input.pop();
                None
            }
            KeyEvent::Enter => {
                let input = self.rename_input.trim().to_string();
                self.rename_input.clear();
                self.mode = AppMode::Normal;
                if input.is_empty() {
                    None
                } else {
                    Some(TuiRuntimeEvent::RenameSubmitted(input))
                }
            }
            KeyEvent::Char(ch) => {
                self.rename_input.push(ch);
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

    fn handle_sort_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Esc => {
                self.mode = AppMode::Normal;
                None
            }
            KeyEvent::Char('n' | 'N') => {
                self.choose_sort_key(SortKey::Name);
                None
            }
            KeyEvent::Char('m' | 'M') => {
                self.choose_sort_key(SortKey::Modified);
                None
            }
            KeyEvent::Char('z' | 'Z') => {
                self.choose_sort_key(SortKey::Size);
                None
            }
            KeyEvent::Char('t' | 'T') => {
                self.choose_sort_key(SortKey::Type);
                None
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

    fn handle_confirm_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Char('y' | 'Y') => {
                let kind = self.confirm_kind.take().unwrap_or(ConfirmKind::Operation);
                self.mode = AppMode::Normal;
                self.confirm_lines.clear();
                match kind {
                    ConfirmKind::Operation => Some(TuiRuntimeEvent::ConfirmAccepted),
                    ConfirmKind::Quit => Some(TuiRuntimeEvent::Action(KeyAction::Quit)),
                }
            }
            KeyEvent::Char('n' | 'N') | KeyEvent::Esc => {
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

    fn set_entries_with_selection(
        &mut self,
        mut entries: Vec<FileEntry>,
        preferred_selection_path: Option<&str>,
    ) {
        sort_entries(&mut entries, self.sort_mode);
        self.entries = entries;
        self.last_file_click = None;
        let selected_index = preferred_selection_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .or_else(|| (!self.entries.is_empty()).then_some(0));
        self.select_index(selected_index);
        self.sync_selection_to_filter();
    }

    fn sort_entries_preserving_selection(&mut self) {
        let selected_path = self.selected_entry().map(|entry| entry.path.clone());
        sort_entries(&mut self.entries, self.sort_mode);
        self.select_index(
            selected_path
                .as_deref()
                .and_then(|path| self.entries.iter().position(|entry| entry.path == path)),
        );
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
        if let Some(state) = self.search_results_state() {
            let status = if state.running { "searching" } else { "search" };
            return Some(format!(
                "{} \"{}\" {} matches, {} dirs",
                status, state.keyword, state.matches, state.scanned_dirs
            ));
        }
        match self.request_state {
            DirectoryRequestState::Loading => Some("Loading...".to_string()),
            DirectoryRequestState::Refreshing => Some("Refreshing...".to_string()),
            DirectoryRequestState::Failed => Some(
                self.notice
                    .as_ref()
                    .map_or_else(|| "Error".to_string(), |notice| format!("Error: {notice}")),
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

fn sort_entries(entries: &mut [FileEntry], mode: SortMode) {
    entries.sort_by(|left, right| match right.is_dir.cmp(&left.is_dir) {
        Ordering::Equal => sort_entry_order(left, right, mode),
        order => order,
    });
}

fn sort_entry_order(left: &FileEntry, right: &FileEntry, mode: SortMode) -> Ordering {
    let order = match mode.key {
        SortKey::Name => name_order(left, right),
        SortKey::Modified => left
            .mtime
            .cmp(&right.mtime)
            .then_with(|| name_order(left, right)),
        SortKey::Size => left
            .size
            .cmp(&right.size)
            .then_with(|| name_order(left, right)),
        SortKey::Type => entry_type_key(left)
            .cmp(&entry_type_key(right))
            .then_with(|| name_order(left, right)),
    };
    match mode.direction {
        SortDirection::Ascending => order,
        SortDirection::Descending => order.reverse(),
    }
}

fn name_order(left: &FileEntry, right: &FileEntry) -> Ordering {
    left.server_filename
        .to_lowercase()
        .cmp(&right.server_filename.to_lowercase())
        .then_with(|| left.server_filename.cmp(&right.server_filename))
        .then_with(|| left.path.cmp(&right.path))
}

fn entry_type_key(entry: &FileEntry) -> String {
    if entry.is_dir {
        return "directory".to_string();
    }
    entry
        .server_filename
        .rsplit_once('.')
        .map_or("", |(_, suffix)| suffix)
        .to_lowercase()
}

const fn default_sort_direction(key: SortKey) -> SortDirection {
    match key {
        SortKey::Modified => SortDirection::Descending,
        SortKey::Name | SortKey::Size | SortKey::Type => SortDirection::Ascending,
    }
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
    transfer: Rect,
    status: Rect,
}

fn full_layout(area: Rect, has_transfer_area: bool) -> FullLayout {
    if has_transfer_area {
        let [header, body, tasks, transfer, status] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(4),
                Constraint::Length(4),
                Constraint::Length(4),
                Constraint::Length(1),
            ])
            .areas(area);
        return FullLayout {
            header,
            body,
            tasks,
            transfer,
            status,
        };
    }

    let [header, body, tasks, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(area);
    FullLayout {
        header,
        body,
        tasks,
        transfer: Rect::default(),
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

fn vertical_scrollbar_rect(inner: Rect) -> Option<Rect> {
    (inner.width > 0 && inner.height > 0)
        .then(|| Rect::new(inner.right().saturating_sub(1), inner.y, 1, inner.height))
}

fn scrollbar_position_for_offset(total: usize, rows: usize, offset: usize) -> usize {
    if total == 0 || rows == 0 {
        return 0;
    }

    let rows = rows.min(total);
    let max_start = total.saturating_sub(rows);
    if max_start == 0 {
        return 0;
    }

    proportional_position(offset.min(max_start), total.saturating_sub(1), max_start)
}

fn scrollbar_offset_at_row(total: usize, rows: usize, area: Rect, row: u16) -> Option<usize> {
    if total == 0 || rows == 0 || total <= rows {
        return None;
    }

    let rows = rows.min(total);
    let max_start = total.saturating_sub(rows);
    if row <= area.y {
        return Some(0);
    }

    let last_row = area.bottom().saturating_sub(1);
    if row >= last_row {
        return Some(max_start);
    }

    let track_len = usize::from(area.height.saturating_sub(2));
    if track_len <= 1 {
        return Some(max_start / 2);
    }

    let track_row =
        usize::from(row.saturating_sub(area.y.saturating_add(1))).min(track_len.saturating_sub(1));
    Some(proportional_position(
        track_row,
        max_start,
        track_len.saturating_sub(1),
    ))
}

fn proportional_position(value: usize, max_value: usize, max_input: usize) -> usize {
    if value == 0 || max_value == 0 || max_input == 0 {
        return 0;
    }

    (((value as u128) * (max_value as u128) + ((max_input as u128) / 2)) / (max_input as u128))
        as usize
}

pub fn render_app(frame: &mut Frame<'_>, app: &TuiApp, preview: Option<PreviewBudget>) {
    frame.render_widget(Block::new().style(surface_style()), frame.area());

    let layout = full_layout(frame.area(), !app.task_items.is_empty());
    let browser = browser_layout(layout.body);
    frame.render_widget(header_widget(app), layout.header);
    render_files_pane(frame, app, browser.files);
    render_preview_pane(frame, app, preview, browser.preview);
    render_tasks_pane(frame, app, layout.tasks);
    render_transfer_pane(frame, app, layout.transfer);
    frame.render_widget(status_widget(app), layout.status);
    if app.help_visible {
        let area = centered_rect(frame.area(), 58, 22);
        frame.render_widget(Clear, area);
        frame.render_widget(help_widget(), area);
    }
    if !app.confirm_lines.is_empty() {
        let body_lines = confirm_body_lines(app).len() as u16;
        let area = centered_rect(frame.area(), 64, body_lines.saturating_add(4).clamp(7, 14));
        frame.render_widget(Clear, area);
        frame.render_widget(confirm_widget(app), area);
    }
    if app.mode == AppMode::Sort {
        let area = centered_rect(frame.area(), 48, 9);
        frame.render_widget(Clear, area);
        frame.render_widget(sort_widget(app), area);
    }
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
        MouseEventKind::Up(MouseButton::Left) => Some(MouseInputKind::LeftRelease),
        MouseEventKind::Drag(MouseButton::Left) => Some(MouseInputKind::LeftDrag),
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

fn surface_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_BG)
}

fn content_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_PANEL)
}

fn meta_value_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_PANEL)
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
        Span::styled(label, Style::new().fg(THEME_MUTED).bg(THEME_PANEL)),
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
        content_line("  f filter  s search : command"),
        content_line("  ? help             Esc close/cancel"),
        content_line("  space mark         u refresh"),
        content_line("  x delete           r rename"),
        content_line("  , sort             c cancel task"),
        content_line("  d download         p preview/open  q quit"),
        content_line("  y/n                confirm / cancel"),
        Line::raw("").style(surface_style()),
        help_section_line("Mouse"),
        content_line("  file click selects double-click opens"),
        content_line("  wheel follows mouse pane  path click jumps"),
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
    let rows = files_visible_rows(area);
    let visible_indices = app.filtered_indices();
    let range = app.visible_entry_range(rows);
    let selected_row = app.selected_index().and_then(|selected| {
        visible_indices[range.clone()]
            .iter()
            .position(|index| *index == selected)
    });
    let mut table_state = TableState::new().with_selected(selected_row);
    frame.render_stateful_widget(files_widget(app, area), area, &mut table_state);

    let total = app.filtered_entry_count();
    if total <= rows || rows == 0 {
        return;
    }

    let range = app.visible_entry_range(rows);
    let mut state = ScrollbarState::new(total)
        .position(scrollbar_position_for_offset(total, rows, range.start))
        .viewport_content_length(rows);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(Style::new().fg(THEME_ACCENT).bg(THEME_PANEL))
            .track_style(Style::new().fg(THEME_BORDER).bg(THEME_PANEL)),
        inner_rect(area),
        &mut state,
    );
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileTableColumns {
    widths: Vec<Constraint>,
    name_width: usize,
    show_mtime: bool,
}

fn files_visible_rows(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(2))
}

fn file_table_columns(inner_width: usize) -> FileTableColumns {
    let show_mtime = inner_width >= 64;
    let size_width = 11;
    let mtime_width = 16;
    let fixed_width = if show_mtime {
        1 + 3 + size_width + mtime_width
    } else {
        1 + 3 + size_width
    };
    let column_count: usize = if show_mtime { 5 } else { 4 };
    let spacing_width = column_count.saturating_sub(1);
    let name_width = inner_width
        .saturating_sub(fixed_width)
        .saturating_sub(spacing_width)
        .max(4);
    let mut widths = vec![
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(name_width as u16),
        Constraint::Length(size_width as u16),
    ];
    if show_mtime {
        widths.push(Constraint::Length(mtime_width as u16));
    }
    FileTableColumns {
        widths,
        name_width,
        show_mtime,
    }
}

fn files_widget(app: &TuiApp, area: Rect) -> Table<'_> {
    let rows = files_visible_rows(area);
    let visible_indices = app.filtered_indices();
    let range = app.visible_entry_range(rows);
    let inner_width = area.width.saturating_sub(2) as usize;
    let columns = file_table_columns(inner_width);
    let items: Vec<Row<'_>> = if range.is_empty() {
        vec![Row::new(vec![Cell::from(
            Line::raw(files_empty_message(app)).style(meta_value_style()),
        )])]
    } else {
        visible_indices[range.clone()]
            .iter()
            .map(|index| {
                let entry = &app.entries()[*index];
                let selected = app.selected_index() == Some(*index);
                let marked = app.selected_paths.contains(&entry.path);
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
                let name_width = columns.name_width;
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
                let mtime_style = if selected {
                    Style::new().fg(THEME_TEXT).bg(row_bg)
                } else {
                    Style::new().fg(THEME_DIM).bg(row_bg)
                };
                let mut cells = vec![
                    Cell::from(Line::from(Span::styled(cursor_marker, cursor_style))),
                    Cell::from(Line::from(Span::styled(mark_marker, mark_style))),
                    Cell::from(Line::from(Span::styled(name, name_style))),
                    Cell::from(Line::from(Span::styled(size, size_style))),
                ];
                if columns.show_mtime {
                    cells.push(Cell::from(Line::from(Span::styled(
                        modified_time_label(entry.mtime),
                        mtime_style,
                    ))));
                }
                Row::new(cells).style(base_style)
            })
            .collect()
    };

    let total = visible_indices.len();
    let title = if let Some(state) = app.search_results_state() {
        let prefix = if state.running { "Search" } else { "Results" };
        if total == 0 {
            format!("{prefix} 0 matches")
        } else if app.filter_input.is_empty() {
            format!(
                "{prefix} {}-{}/{} matches",
                range.start + 1,
                range.end,
                total
            )
        } else {
            format!(
                "{prefix} {}-{}/{} of {} matches",
                range.start + 1,
                range.end,
                total,
                app.entries().len()
            )
        }
    } else if total == 0 {
        if app.filter_input.is_empty() {
            "Files 0 items".to_string()
        } else {
            format!("Files 0/{} items", app.entries().len())
        }
    } else if app.filter_input.is_empty() {
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
    let widths = if range.is_empty() {
        vec![Constraint::Min(1)]
    } else {
        columns.widths
    };
    Table::new(items, widths)
        .block(themed_block(title, app.focus == PanelFocus::Files))
        .style(surface_style())
        .column_spacing(1)
        .highlight_spacing(HighlightSpacing::Never)
        .row_highlight_style(
            Style::new()
                .fg(THEME_TEXT)
                .bg(THEME_SELECTED)
                .add_modifier(Modifier::BOLD),
        )
}

fn preview_widget(app: &TuiApp, preview: Option<PreviewBudget>, area: Rect) -> Paragraph<'_> {
    let inner = inner_rect(area);
    let inner_width = usize::from(inner.width);
    let selected = app.selected_entry();
    let title = if app.preview_title.is_empty() {
        selected
            .map(|entry| entry.server_filename.as_str())
            .or(app.notice.as_deref())
            .unwrap_or("metadata")
    } else {
        app.preview_title.as_str()
    };
    let budget = preview.unwrap_or(app.preview_budget);

    let mut lines = selected.map_or_else(
        || {
            let name_width = detail_value_width(inner_width, "Name:");
            vec![
                detail_line("Name:", marquee_cells(title, name_width, app.marquee_tick)),
                detail_line("Kind:", "none".to_string()),
                detail_line("Cache:", app.cache_source.user_label().to_string()),
            ]
        },
        |entry| {
            preview_detail_lines(
                entry,
                app.cache_source,
                budget,
                inner_width,
                app.marquee_tick,
            )
        },
    );
    lines.push(Line::raw("").style(surface_style()));
    if budget.kind == PreviewKind::Video && !app.preview_lines.is_empty() {
        push_plain_lines(&mut lines, &app.preview_lines, "", content_style());
        lines.push(Line::raw("").style(surface_style()));
        lines.push(detail_line(
            "Buffer:",
            format!(
                "{} / {}",
                bytes_label(budget.fetched_bytes),
                bytes_label(budget.memory_limit_bytes)
            ),
        ));
    } else if budget.kind == PreviewKind::Text && !app.preview_lines.is_empty() {
        lines.truncate(4);
        lines.push(detail_line(
            "Preview:",
            format!("text · {} fetched", bytes_label(budget.fetched_bytes)),
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
        .block(themed_block("Preview", app.focus == PanelFocus::Preview))
        .style(surface_style())
        .scroll((scroll, 0))
        .wrap(Wrap { trim: true })
}

fn render_preview_pane(
    frame: &mut Frame<'_>,
    app: &TuiApp,
    preview: Option<PreviewBudget>,
    area: Rect,
) {
    frame.render_widget(preview_widget(app, preview, area), area);

    let rows = usize::from(inner_rect(area).height);
    let total = app.preview_render_line_count();
    if rows == 0 || total <= rows {
        return;
    }

    let mut state = ScrollbarState::new(total)
        .position(scrollbar_position_for_offset(
            total,
            rows,
            app.preview_scroll_offset,
        ))
        .viewport_content_length(rows);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(Style::new().fg(THEME_ACCENT).bg(THEME_PANEL))
            .track_style(Style::new().fg(THEME_BORDER).bg(THEME_PANEL)),
        inner_rect(area),
        &mut state,
    );
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

fn render_tasks_pane(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    if app.task_items.is_empty() {
        frame.render_widget(tasks_widget(app), area);
        return;
    }

    frame.render_widget(themed_block("Tasks", false), area);
    let inner = inner_rect(area);
    if inner.height == 0 {
        return;
    }

    frame.render_widget(task_table_widget(app, usize::from(inner.height)), inner);
}

fn render_transfer_pane(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let Some(task) = app.task_items.first() else {
        return;
    };
    if area.is_empty() {
        return;
    }

    frame.render_widget(themed_block("Transfer", false), area);
    let inner = inner_rect(area);
    if inner.height == 0 {
        return;
    }

    let [progress_area, speed_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .areas(inner);
    let speed_history = app
        .task_speed_history
        .get(&task.id)
        .map_or(&[][..], Vec::as_slice);
    render_task_progress(frame, task, progress_area);
    render_task_speed(frame, task, speed_history, speed_area);
}

fn task_table_widget(app: &TuiApp, rows: usize) -> Table<'_> {
    let table_rows = app.task_items.iter().take(rows).map(|task| {
        let state_style = match task.state.as_str() {
            "failed" | "cancelled" => Style::new().fg(THEME_ERROR).bg(THEME_PANEL),
            "done" => Style::new().fg(THEME_SUCCESS).bg(THEME_PANEL),
            "running" => Style::new().fg(THEME_ACCENT).bg(THEME_PANEL),
            _ => meta_value_style(),
        };
        Row::new(vec![
            Cell::from(Line::raw(format!("#{}", task.id)).style(meta_value_style())),
            Cell::from(Line::raw(task.kind.clone()).style(meta_value_style())),
            Cell::from(Line::raw(task.state.clone()).style(state_style)),
            Cell::from(Line::raw(task.label.clone()).style(content_style())),
        ])
        .style(content_style())
    });
    Table::new(
        table_rows,
        [
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Min(12),
        ],
    )
    .column_spacing(1)
    .highlight_spacing(HighlightSpacing::Never)
    .style(surface_style())
}

fn render_task_progress(frame: &mut Frame<'_>, task: &TaskPanelItem, area: Rect) {
    if area.is_empty() {
        return;
    }
    if let Some(total) = task.bytes_total.filter(|total| *total > 0) {
        let ratio = (task.bytes_done as f64 / total as f64).clamp(0.0, 1.0);
        let label = format!(
            "{}% {} / {}",
            (ratio * 100.0).round() as u64,
            bytes_label(task.bytes_done),
            bytes_label(total)
        );
        frame.render_widget(
            Gauge::default()
                .ratio(ratio)
                .label(label)
                .style(content_style())
                .gauge_style(Style::new().fg(THEME_ACCENT).bg(THEME_MARKED)),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(Line::raw(format!(
                "progress {} / ?",
                bytes_label(task.bytes_done)
            )))
            .style(content_style()),
            area,
        );
    }
}

fn render_task_speed(frame: &mut Frame<'_>, task: &TaskPanelItem, history: &[u64], area: Rect) {
    if area.is_empty() {
        return;
    }
    let [label_area, spark_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(18), Constraint::Min(1)])
        .areas(area);
    frame.render_widget(
        Paragraph::new(Line::raw(format!(
            "{}/s",
            bytes_label(task.speed_bytes_per_sec)
        )))
        .style(meta_value_style()),
        label_area,
    );
    if history.is_empty() {
        return;
    }
    frame.render_widget(
        Sparkline::default()
            .data(history.iter().copied())
            .style(Style::new().fg(THEME_SUCCESS).bg(THEME_PANEL)),
        spark_area,
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
        Style::new().fg(THEME_DIRECTORY).bg(THEME_PANEL),
    );
    if let Some(notice) = app.notice.as_deref() {
        lines.push(
            Line::raw(notice.to_string()).style(Style::new().fg(THEME_WARNING).bg(THEME_PANEL)),
        );
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
        lines.push(Line::raw(line.clone()).style(style));
    }
    if body_lines.len() > 9 {
        lines.push(Line::raw(format!("+{} more", body_lines.len() - 9)).style(meta_value_style()));
    }
    lines.push(Line::raw("").style(surface_style()));
    lines.push(Line::from(vec![
        Span::styled("[y] confirm", confirm_style()),
        Span::styled("    ", surface_style()),
        Span::styled(
            "[n] cancel",
            Style::new()
                .fg(THEME_WARNING)
                .bg(THEME_PANEL)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    Paragraph::new(lines)
        .block(
            Block::bordered()
                .style(surface_style())
                .border_style(Style::new().fg(THEME_ERROR).bg(THEME_BG))
                .title(Line::from(Span::styled(
                    match app.confirm_kind {
                        Some(ConfirmKind::Quit) => "Quit?",
                        Some(ConfirmKind::Operation) | None => "Confirm",
                    },
                    Style::new()
                        .fg(THEME_ERROR)
                        .bg(THEME_BG)
                        .add_modifier(Modifier::BOLD),
                ))),
        )
        .style(surface_style())
        .wrap(Wrap { trim: true })
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

fn sort_widget(app: &TuiApp) -> Paragraph<'_> {
    let options = [
        (SortKey::Name, "n", "Name"),
        (SortKey::Modified, "m", "Modified"),
        (SortKey::Size, "z", "Size"),
        (SortKey::Type, "t", "Type"),
    ];
    let mut lines = vec![Line::raw(app.sort_mode.label()).style(confirm_style())];
    lines.push(Line::raw("").style(surface_style()));
    for (key, shortcut, label) in options {
        let active = app.sort_mode.key == key;
        let marker = if active {
            app.sort_mode.direction.marker()
        } else {
            "select"
        };
        let style = if active {
            confirm_style()
        } else {
            content_style()
        };
        lines.push(Line::raw(format!("[{shortcut}] {label:<8} {marker}")).style(style));
    }
    lines.push(Line::raw("").style(surface_style()));
    lines.push(Line::raw("Esc cancel").style(meta_value_style()));

    Paragraph::new(lines)
        .block(
            Block::bordered()
                .style(surface_style())
                .border_style(Style::new().fg(THEME_ACCENT).bg(THEME_BG))
                .title(Line::from(Span::styled(
                    "Sort",
                    Style::new()
                        .fg(THEME_ACCENT)
                        .bg(THEME_BG)
                        .add_modifier(Modifier::BOLD),
                ))),
        )
        .style(surface_style())
}

fn status_widget(app: &TuiApp) -> Paragraph<'_> {
    if app.mode == AppMode::Command {
        return input_status_widget(":", &app.command_input);
    }
    if app.mode == AppMode::Filter {
        return input_status_widget("f", &app.filter_input);
    }
    if app.mode == AppMode::Search {
        return input_status_widget("s", &app.search_input);
    }
    if app.mode == AppMode::Rename {
        return input_status_widget("rename ", &app.rename_input);
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
            app.sort_mode.label(),
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

fn files_empty_message(app: &TuiApp) -> String {
    if !app.filter_input.is_empty() {
        return format!("No matches for \"{}\"", app.filter_input);
    }
    if let Some(state) = app.search_results_state() {
        return if state.running {
            format!("Searching \"{}\"...", state.keyword)
        } else {
            format!("No matches for \"{}\"", state.keyword)
        };
    }
    match app.request_state {
        DirectoryRequestState::Loading => "Loading...".to_string(),
        DirectoryRequestState::Refreshing => "Refreshing cached entries...".to_string(),
        DirectoryRequestState::Failed => app.notice.as_deref().map_or_else(
            || "Could not load directory".to_string(),
            |notice| format!("Error: {notice}"),
        ),
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
        DirectoryRequestState::Failed => app.notice.as_deref().map(|notice| {
            Line::raw(format!("Error: {notice}")).style(
                Style::new()
                    .fg(request_state_color(app.request_state))
                    .bg(THEME_PANEL),
            )
        }),
        DirectoryRequestState::Idle => (app.background_tasks > 0)
            .then(|| {
                Line::raw(format!(
                    "{} {} running",
                    app.background_tasks,
                    pluralize(app.background_tasks, "task", "tasks")
                ))
                .style(meta_value_style())
            })
            .or_else(|| {
                app.search_results_state().map(|state| {
                    Line::raw(format!(
                        "{} matches for \"{}\" in {} ({} dirs, {} entries)",
                        state.matches,
                        state.keyword,
                        state.root_path,
                        state.scanned_dirs,
                        state.scanned_entries
                    ))
                    .style(meta_value_style())
                })
            }),
    }
}

const fn pluralize(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use baidupcsr_pcs::FileEntry;
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
}
