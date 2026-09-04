use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppMode {
    Normal,
    Filter,
    Search,
    Rename,
    Sort,
    Command,
    Confirm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelFocus {
    Files,
    Preview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfirmKind {
    Operation,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputPopupAnchor {
    TopCenter,
    SelectedFile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputPopup<'a> {
    pub(crate) title: &'static str,
    pub(crate) prompt: &'static str,
    pub(crate) input: &'a str,
    pub(crate) anchor: InputPopupAnchor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FileClick {
    pub(crate) index: usize,
    pub(crate) tick: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScrollbarDragTarget {
    Files,
    Preview,
}

impl AppMode {
    pub(crate) const fn label(self) -> &'static str {
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
pub(crate) enum SortKey {
    Name,
    Modified,
    Size,
    Type,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SortMode {
    pub(crate) key: SortKey,
    pub(crate) direction: SortDirection,
}

impl SortKey {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Modified => "modified",
            Self::Size => "size",
            Self::Type => "type",
        }
    }
}

impl SortDirection {
    pub(crate) const fn marker(self) -> &'static str {
        match self {
            Self::Ascending => "asc",
            Self::Descending => "desc",
        }
    }
}

impl SortMode {
    pub(crate) const fn default() -> Self {
        Self {
            key: SortKey::Name,
            direction: SortDirection::Ascending,
        }
    }

    pub(crate) fn label(self) -> String {
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
pub(crate) enum DirectoryRequestState {
    Idle,
    Loading,
    Refreshing,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SearchResultsState {
    pub(crate) root_path: String,
    pub(crate) keyword: String,
    pub(crate) restore_entries: Vec<FileEntry>,
    pub(crate) restore_selection_path: Option<String>,
    pub(crate) scanned_dirs: usize,
    pub(crate) scanned_entries: usize,
    pub(crate) matches: usize,
    pub(crate) running: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileListMode {
    Directory,
    SearchResults(SearchResultsState),
}

impl DirectoryRequestState {
    pub(crate) const fn user_label(self) -> &'static str {
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
    pub(crate) const fn user_label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Disk => "disk",
            Self::Network => "network",
        }
    }
}

pub(crate) const fn key_action_for(event: KeyEvent) -> Option<KeyAction> {
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
    pub(crate) path: String,
    pub(crate) entries: Vec<FileEntry>,
    pub(crate) selected_index: Option<usize>,
    pub(crate) mode: AppMode,
    pub(crate) background_tasks: usize,
    pub(crate) notice: Option<String>,
    pub(crate) request_state: DirectoryRequestState,
    pub(crate) cache_age_seconds: Option<u64>,
    pub(crate) cache_source: CacheSource,
    pub(crate) preview_title: String,
    pub(crate) preview_lines: Vec<String>,
    pub(crate) preview_budget: PreviewStatus,
    pub(crate) task_items: Vec<TaskPanelItem>,
    pub(crate) task_speed_history: BTreeMap<u64, Vec<u64>>,
    pub(crate) preload_lines: Vec<String>,
    pub(crate) command_input: String,
    pub(crate) filter_input: String,
    /// Indices matching `filter_input`, kept stable between filter/list changes.
    ///
    /// Rendering asks for the visible indices several times per frame.  Keeping
    /// this derived list on the app avoids repeatedly allocating and lowercasing
    /// every filename on the hot render path.
    filtered_indices_cache: Vec<usize>,
    pub(crate) search_input: String,
    pub(crate) rename_input: String,
    pub(crate) sort_mode: SortMode,
    pub(crate) file_list_mode: FileListMode,
    pub(crate) selected_paths: Vec<String>,
    pub(crate) confirm_lines: Vec<String>,
    pub(crate) confirm_kind: Option<ConfirmKind>,
    pub(crate) help_visible: bool,
    pub(crate) pending_g: bool,
    pub(crate) scroll_offset: usize,
    pub(crate) preview_scroll_offset: usize,
    pub(crate) marquee_tick: u64,
    pub(crate) last_file_click: Option<FileClick>,
    pub(crate) scrollbar_drag: Option<ScrollbarDragTarget>,
    pub(crate) focus: PanelFocus,
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
            filtered_indices_cache: Vec::new(),
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
        self.rebuild_filtered_indices();
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

    pub(crate) fn begin_rename_input(&mut self) {
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

    pub(crate) fn begin_sort_menu(&mut self) {
        self.mode = AppMode::Sort;
    }

    pub(crate) fn choose_sort_key(&mut self, key: SortKey) {
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

    pub fn finish_preview_text(&mut self, title: &str, content: &str, budget: PreviewStatus) {
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
        budget: PreviewStatus,
    ) {
        self.preview_title = title.to_string();
        self.preview_lines = vec![format!("metadata: {}", reason.into())];
        self.preview_budget = PreviewStatus { kind, ..budget };
        self.preview_scroll_offset = 0;
    }

    pub fn finish_preview_error(&mut self, title: &str, message: impl Into<String>) {
        self.preview_title = title.to_string();
        self.preview_lines = vec![format!("preview_error: {}", message.into())];
        self.preview_budget = default_preview_budget();
        self.preview_scroll_offset = 0;
    }

    pub fn finish_preview_video(&mut self, title: &str, lines: Vec<String>, budget: PreviewStatus) {
        self.preview_title = title.to_string();
        self.preview_lines = lines;
        self.preview_budget = PreviewStatus {
            kind: PreviewKind::Video,
            ..budget
        };
        self.preview_scroll_offset = 0;
    }

    pub fn finish_preview_image_metadata(
        &mut self,
        title: &str,
        lines: Vec<String>,
        budget: PreviewStatus,
    ) {
        self.preview_title = title.to_string();
        self.preview_lines = lines;
        self.preview_budget = PreviewStatus {
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

    pub(crate) fn filtered_entry_count(&self) -> usize {
        self.filtered_indices().len()
    }

    pub fn selected_paths(&self) -> &[String] {
        &self.selected_paths
    }

    pub(crate) fn search_results_state(&self) -> Option<&SearchResultsState> {
        match &self.file_list_mode {
            FileListMode::Directory => None,
            FileListMode::SearchResults(state) => Some(state),
        }
    }

    pub(crate) fn search_results_state_mut(&mut self) -> Option<&mut SearchResultsState> {
        match &mut self.file_list_mode {
            FileListMode::Directory => None,
            FileListMode::SearchResults(state) => Some(state),
        }
    }

    pub(crate) fn take_search_results_state(&mut self) -> Option<SearchResultsState> {
        match std::mem::replace(&mut self.file_list_mode, FileListMode::Directory) {
            FileListMode::Directory => None,
            FileListMode::SearchResults(state) => Some(state),
        }
    }

    pub(crate) fn update_search_notice(&mut self) {
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

    pub(crate) fn visible_entry_range(&self, rows: usize) -> Range<usize> {
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

    pub(crate) fn move_to_top(&mut self) {
        let indices = self.filtered_indices();
        let Some(index) = indices.first().copied() else {
            return;
        };
        self.select_index(Some(index));
        self.scroll_offset = 0;
    }

    pub(crate) fn move_to_bottom(&mut self) {
        let (index, count) = {
            let indices = self.filtered_indices();
            let Some(index) = indices.last().copied() else {
                return;
            };
            (index, indices.len())
        };
        self.select_index(Some(index));
        self.scroll_offset = count.saturating_sub(1);
    }

    pub(crate) fn move_visible_by(&mut self, delta: isize) {
        let indices = self.filtered_indices();
        let Some(position) = self.selected_visible_position(indices) else {
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

    pub(crate) fn scroll_files_by_wheel(&mut self, delta: isize) -> bool {
        let old_offset = self.scroll_offset;
        let old_selection = self.selected_index;
        self.move_visible_by(delta);
        old_offset != self.scroll_offset || old_selection != self.selected_index
    }

    pub(crate) fn scroll_files_to_offset(&mut self, offset: usize, rows: usize) -> bool {
        let old_offset = self.scroll_offset;
        let old_selection = self.selected_index;
        let (entry_count, selected_position) = {
            let indices = self.filtered_indices();
            (indices.len(), self.selected_visible_position(indices))
        };
        if entry_count == 0 {
            self.scroll_offset = 0;
            self.select_index(None);
            return old_offset != self.scroll_offset || old_selection != self.selected_index;
        }
        if rows == 0 {
            return false;
        }

        let rows = rows.min(entry_count);
        let start = offset.min(entry_count.saturating_sub(rows));
        let end = start.saturating_add(rows);

        let target_position = match selected_position {
            Some(position) if position >= start && position < end => position,
            Some(position) if position < start => start,
            Some(_) => end.saturating_sub(1),
            None => start,
        };
        let target_index = self.filtered_indices().get(target_position).copied();
        self.scroll_offset = start;
        self.select_index(target_index);

        old_offset != self.scroll_offset || old_selection != self.selected_index
    }

    pub(crate) fn scroll_preview_by(&mut self, delta: isize, rows: usize) -> bool {
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

    pub(crate) fn scroll_preview_to_offset(&mut self, offset: usize, rows: usize) -> bool {
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

    pub(crate) fn handle_files_scrollbar_pointer(
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

    pub(crate) fn handle_preview_scrollbar_pointer(
        &mut self,
        row: u16,
        scrollbar: Rect,
        rows: usize,
    ) {
        let Some(offset) =
            scrollbar_offset_at_row(self.preview_render_line_count(), rows, scrollbar, row)
        else {
            return;
        };
        self.focus = PanelFocus::Preview;
        self.scroll_preview_to_offset(offset, rows);
    }

    pub(crate) fn select_index(&mut self, index: Option<usize>) {
        if self.selected_index != index {
            self.preview_scroll_offset = 0;
            self.marquee_tick = 0;
            self.last_file_click = None;
        }
        self.selected_index = index;
    }

    pub(crate) fn preview_render_line_count(&self) -> usize {
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

    pub(crate) fn handle_normal_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
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
                self.rebuild_filtered_indices();
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

    pub(crate) fn handle_command_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
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

    pub(crate) fn handle_filter_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
        match event {
            KeyEvent::Esc => {
                self.mode = AppMode::Normal;
                self.filter_input.clear();
                self.rebuild_filtered_indices();
                self.sync_selection_to_filter();
                None
            }
            KeyEvent::Backspace => {
                self.filter_input.pop();
                self.rebuild_filtered_indices();
                self.sync_selection_to_filter();
                None
            }
            KeyEvent::Enter => {
                self.mode = AppMode::Normal;
                None
            }
            KeyEvent::Char(ch) => {
                self.filter_input.push(ch);
                self.rebuild_filtered_indices();
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

    pub(crate) fn handle_search_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
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

    pub(crate) fn handle_rename_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
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

    pub(crate) fn handle_sort_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
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

    pub(crate) fn handle_confirm_key(&mut self, event: KeyEvent) -> Option<TuiRuntimeEvent> {
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

    pub(crate) fn toggle_selected_path(&mut self) {
        let Some(path) = self.selected_entry().map(|entry| entry.path.clone()) else {
            return;
        };
        if let Some(index) = self.selected_paths.iter().position(|item| item == &path) {
            self.selected_paths.remove(index);
        } else {
            self.selected_paths.push(path);
        }
    }

    pub(crate) fn set_entries_with_selection(
        &mut self,
        mut entries: Vec<FileEntry>,
        preferred_selection_path: Option<&str>,
    ) {
        sort_entries(&mut entries, self.sort_mode);
        self.entries = entries;
        self.rebuild_filtered_indices();
        self.last_file_click = None;
        let selected_index = preferred_selection_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .or_else(|| (!self.entries.is_empty()).then_some(0));
        self.select_index(selected_index);
        self.sync_selection_to_filter();
    }

    pub(crate) fn sort_entries_preserving_selection(&mut self) {
        let selected_path = self.selected_entry().map(|entry| entry.path.clone());
        sort_entries(&mut self.entries, self.sort_mode);
        self.rebuild_filtered_indices();
        self.select_index(
            selected_path
                .as_deref()
                .and_then(|path| self.entries.iter().position(|entry| entry.path == path)),
        );
    }

    pub(crate) fn filtered_indices(&self) -> &[usize] {
        &self.filtered_indices_cache
    }

    fn rebuild_filtered_indices(&mut self) {
        let needle = self.filter_input.trim().to_lowercase();
        self.filtered_indices_cache = if needle.is_empty() {
            (0..self.entries.len()).collect()
        } else {
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
        };
    }

    pub(crate) fn selected_visible_position(&self, indices: &[usize]) -> Option<usize> {
        let selected = self.selected_index?;
        indices.iter().position(|index| *index == selected)
    }

    pub(crate) fn sync_selection_to_filter(&mut self) {
        let indices = self.filtered_indices();
        if indices.is_empty() {
            self.select_index(None);
            self.scroll_offset = 0;
            return;
        }
        if let Some(position) = self.selected_visible_position(indices) {
            self.scroll_offset = position.saturating_sub(1);
            return;
        }
        self.select_index(indices.first().copied());
        self.scroll_offset = 0;
    }

    pub(crate) fn begin_quit_confirm(&mut self) {
        self.mode = AppMode::Confirm;
        self.confirm_kind = Some(ConfirmKind::Quit);
        self.confirm_lines = vec![format!(
            "Quit with {} {} still running?",
            self.background_tasks,
            if self.background_tasks == 1 {
                "task"
            } else {
                "tasks"
            }
        )];
    }

    pub(crate) fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            PanelFocus::Files => PanelFocus::Preview,
            PanelFocus::Preview => PanelFocus::Files,
        };
    }

    pub(crate) fn position_label(&self) -> String {
        let indices = self.filtered_indices();
        if indices.is_empty() {
            return "0/0".to_string();
        }
        let position = self.selected_visible_position(indices).unwrap_or(0);
        format!("{}/{}", position.saturating_add(1), indices.len())
    }

    pub(crate) fn status_hint(&self) -> Option<String> {
        if self.is_text_input_mode() {
            return None;
        }
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
                if self.background_tasks == 1 {
                    "task"
                } else {
                    "tasks"
                }
            )),
            DirectoryRequestState::Idle => None,
        }
    }

    pub(crate) const fn is_text_input_mode(&self) -> bool {
        matches!(
            self.mode,
            AppMode::Command | AppMode::Filter | AppMode::Search | AppMode::Rename
        )
    }
}

pub(crate) fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

pub(crate) fn sort_entries(entries: &mut [FileEntry], mode: SortMode) {
    entries.sort_by(|left, right| match right.is_dir.cmp(&left.is_dir) {
        Ordering::Equal => sort_entry_order(left, right, mode),
        order => order,
    });
}

pub(crate) fn sort_entry_order(left: &FileEntry, right: &FileEntry, mode: SortMode) -> Ordering {
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

pub(crate) fn name_order(left: &FileEntry, right: &FileEntry) -> Ordering {
    left.server_filename
        .to_lowercase()
        .cmp(&right.server_filename.to_lowercase())
        .then_with(|| left.server_filename.cmp(&right.server_filename))
        .then_with(|| left.path.cmp(&right.path))
}

pub(crate) fn entry_type_key(entry: &FileEntry) -> String {
    if entry.is_dir {
        return "directory".to_string();
    }
    entry
        .server_filename
        .rsplit_once('.')
        .map_or("", |(_, suffix)| suffix)
        .to_lowercase()
}

pub(crate) const fn default_sort_direction(key: SortKey) -> SortDirection {
    match key {
        SortKey::Modified => SortDirection::Descending,
        SortKey::Name | SortKey::Size | SortKey::Type => SortDirection::Ascending,
    }
}

pub(crate) fn visible_range_from_offset(total: usize, offset: usize, rows: usize) -> Range<usize> {
    if total == 0 || rows == 0 {
        return 0..0;
    }

    let rows = rows.min(total);
    let start = offset.min(total.saturating_sub(rows));

    start..start.saturating_add(rows)
}
