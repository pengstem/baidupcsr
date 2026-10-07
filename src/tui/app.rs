//! UI state and input handling. Pure: side effects are returned as [`Cmd`]s
//! for the controller to perform.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::thread::ThreadProtocol;

use super::column::Column;
use super::keymap::{Action, action_for};
use super::line_edit::{Edit, LineEdit};
use super::theme::Theme;
use crate::app::directory::{DirectoryCache, SortKey, SortMode, sort_entries};
use crate::app::ops::{self, Command, CommandContext, Operation, SearchMode};
use crate::app::remote_path;
use crate::app::task::{TaskId, TaskList};
use crate::pcs::{FileEntry, Quota};

const WHEEL_STEP: isize = 3;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Side effects requested by the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// The current directory changed: load it and its parent.
    Navigate,
    /// The entry under the cursor changed: update the preview.
    SelectionChanged,
    Refresh,
    /// Play or open a file.
    Open(FileEntry),
    Download(Vec<FileEntry>),
    Run(Operation),
    Search {
        keyword: String,
        mode: SearchMode,
    },
    Cancel(TaskId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Files,
    Preview,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Overlay {
    None,
    Help,
    Sort,
    Tasks { cursor: usize },
    Confirm(Operation),
    ConfirmQuit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputKind {
    Filter,
    Search,
    Command,
    Rename,
    Mkdir,
}

impl InputKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Filter => "FILTER",
            Self::Search => "SEARCH",
            Self::Command => "COMMAND",
            Self::Rename => "RENAME",
            Self::Mkdir => "MKDIR",
        }
    }
}

pub struct Input {
    pub kind: InputKind,
    pub edit: LineEdit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewBody {
    Empty,
    Loading,
    Text(Arc<str>),
    Image(String),
    Note(String),
    Error(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ImageState {
    #[default]
    None,
    Loading,
    Ready,
    Failed(String),
}

pub struct Preview {
    /// Path of the entry this preview belongs to.
    pub path: String,
    pub body: PreviewBody,
    pub scroll: usize,
    pub image_state: ImageState,
    /// Picture for image files; resized and encoded off-thread. `None` when
    /// the terminal could not be queried (and in tests).
    pub image: Option<ThreadProtocol>,
}

pub struct SearchView {
    pub root: String,
    pub keyword: String,
    pub entries: Vec<FileEntry>,
    pub running: bool,
}

pub struct Clipboard {
    pub paths: Vec<String>,
    pub cut: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastLevel {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub level: ToastLevel,
    pub text: String,
    expires: Instant,
}

/// Screen areas from the last frame, for mouse hit-testing.
#[derive(Clone, Debug, Default)]
pub struct Regions {
    pub parent: Rect,
    pub files: Rect,
    pub preview: Rect,
    /// Clickable breadcrumb segments and the directory each opens.
    pub crumbs: Vec<(Rect, String)>,
}

pub struct App {
    pub cache: DirectoryCache,
    pub cwd: String,
    pub search: Option<SearchView>,
    pub column: Column,
    pub marks: HashSet<String>,
    /// Anchor position of an active visual (range) selection.
    pub visual: Option<usize>,
    pub clipboard: Option<Clipboard>,
    pub preview: Preview,
    pub focus: Focus,
    pub overlay: Overlay,
    pub input: Option<Input>,
    pub tasks: TaskList,
    pub toasts: Vec<Toast>,
    pub quota: Option<Quota>,
    pub theme: Theme,
    pub regions: Regions,
    /// When the cursor last moved, for the long-name marquee.
    pub cursor_moved_at: Instant,
    pub started: Instant,
    pub quit: bool,
    /// Last selected child per directory, restored on return.
    remembered: HashMap<String, String>,
    pending: Option<Action>,
    last_click: Option<(Instant, Position)>,
}

impl App {
    pub fn new(cwd: &str, theme: Theme) -> Self {
        let now = Instant::now();
        Self {
            cache: DirectoryCache::default(),
            cwd: remote_path::normalize(cwd),
            search: None,
            column: Column::default(),
            marks: HashSet::new(),
            visual: None,
            clipboard: None,
            preview: Preview {
                path: String::new(),
                body: PreviewBody::Empty,
                scroll: 0,
                image_state: ImageState::None,
                image: None,
            },
            focus: Focus::Files,
            overlay: Overlay::None,
            input: None,
            tasks: TaskList::default(),
            toasts: Vec::new(),
            quota: None,
            theme,
            regions: Regions::default(),
            cursor_moved_at: now,
            started: now,
            quit: false,
            remembered: HashMap::new(),
            pending: None,
            last_click: None,
        }
    }

    /// Entries shown in the middle column.
    pub fn entries(&self) -> &[FileEntry] {
        shown(self.search.as_ref(), &self.cache, &self.cwd)
    }

    pub fn selected(&self) -> Option<&FileEntry> {
        self.column.selected(self.entries())
    }

    pub fn selected_path(&self) -> Option<&str> {
        self.selected().map(|entry| entry.path.as_str())
    }

    pub fn is_selected(&self, path: &str) -> bool {
        self.selected_path() == Some(path)
    }

    /// The directory the parent column lists, and the entry it highlights.
    pub fn parent_column(&self) -> Option<(String, Option<&str>)> {
        match &self.search {
            Some(search) => Some((search.root.clone(), None)),
            None => remote_path::parent(&self.cwd).map(|parent| (parent, Some(self.cwd.as_str()))),
        }
    }

    pub fn is_marked(&self, position: usize, path: &str) -> bool {
        self.marks.contains(path)
            || self.visual.is_some_and(|anchor| {
                let cursor = self.column.cursor();
                (anchor.min(cursor)..=anchor.max(cursor)).contains(&position)
            })
    }

    pub fn marked_count(&self) -> usize {
        self.marks.len()
            + self.visual.map_or(0, |anchor| {
                let cursor = self.column.cursor();
                anchor.abs_diff(cursor) + 1
            })
    }

    /// Entries an action applies to: marked ones, else the cursor's.
    fn target_entries(&mut self) -> Vec<FileEntry> {
        self.commit_visual();
        if self.marks.is_empty() {
            return self.selected().cloned().into_iter().collect();
        }
        let mut marked: Vec<FileEntry> = self
            .entries()
            .iter()
            .filter(|entry| self.marks.contains(&entry.path))
            .cloned()
            .collect();
        // Marks may come from other directories.
        let listed: HashSet<&str> = marked.iter().map(|entry| entry.path.as_str()).collect();
        let elsewhere: Vec<FileEntry> = self
            .marks
            .iter()
            .filter(|path| !listed.contains(path.as_str()))
            .map(|path| FileEntry {
                path: path.clone(),
                server_filename: remote_path::file_name(path).to_string(),
                ..FileEntry::default()
            })
            .collect();
        marked.extend(elsewhere);
        marked
    }

    fn target_paths(&mut self) -> Vec<String> {
        self.target_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect()
    }

    fn commit_visual(&mut self) {
        let Some(anchor) = self.visual.take() else {
            return;
        };
        let cursor = self.column.cursor();
        let range = anchor.min(cursor)..anchor.max(cursor) + 1;
        let entries = shown(self.search.as_ref(), &self.cache, &self.cwd);
        for &index in self.column.entries_in(range) {
            self.marks.insert(entries[index].path.clone());
        }
    }

    pub fn toast(&mut self, level: ToastLevel, text: impl Into<String>) {
        let ttl = if level == ToastLevel::Error { 6 } else { 3 };
        self.toasts.push(Toast {
            level,
            text: text.into(),
            expires: Instant::now() + Duration::from_secs(ttl),
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    pub fn is_loading(&self) -> bool {
        self.search.as_ref().is_some_and(|search| search.running)
            || self
                .cache
                .get(&self.cwd)
                .is_none_or(|listing| listing.refreshing)
            || self.preview.body == PreviewBody::Loading
            || self.preview.image_state == ImageState::Loading
    }

    /// Whether the screen changes on its own (spinners, toasts).
    pub fn animating(&self) -> bool {
        !self.toasts.is_empty() || self.is_loading() || self.tasks.has_active()
    }

    /// Expire toasts; returns whether anything changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|toast| toast.expires > now);
        before != self.toasts.len() || self.animating()
    }

    pub fn spinner_frame(&self) -> usize {
        (self.started.elapsed().as_millis() / 80) as usize
    }

    /// Open `path` as the current directory.
    pub fn enter_dir(&mut self, path: &str) -> Cmd {
        let path = remote_path::normalize(path);
        if self.search.is_none()
            && let Some(selected) = self.selected_path().map(str::to_owned)
        {
            self.remembered.insert(self.cwd.clone(), selected);
        }
        if let Some(child) = remote_path::child_toward(&path, &self.cwd) {
            self.remembered.insert(path.clone(), child);
        }
        self.cwd = path;
        self.search = None;
        self.visual = None;
        self.focus = Focus::Files;
        self.reset_column();
        self.cursor_moved_at = Instant::now();
        Cmd::Navigate
    }

    /// Show the current directory with its remembered selection.
    fn reset_column(&mut self) {
        let select = self.remembered.get(&self.cwd).map(String::as_str);
        self.column.reset(self.cache.entries(&self.cwd), select);
    }

    /// A listing changed; returns whether the visible column did.
    pub fn listing_updated(&mut self, path: &str) -> bool {
        if self.search.is_some() || path != self.cwd {
            return false;
        }
        let selected = self
            .selected_path()
            .or_else(|| self.remembered.get(path).map(String::as_str))
            .map(str::to_owned);
        self.column
            .rebuild(self.cache.entries(&self.cwd), selected.as_deref());
        true
    }

    pub fn set_preview(&mut self, path: &str, body: PreviewBody) {
        if self.preview.path != path {
            self.preview.scroll = 0;
            self.preview.path = path.to_string();
            self.set_preview_image(ImageState::None, None);
        }
        self.preview.body = body;
    }

    pub fn set_preview_image(&mut self, state: ImageState, picture: Option<StatefulProtocol>) {
        self.preview.image_state = state;
        if let Some(image) = &mut self.preview.image {
            match picture {
                Some(picture) => image.replace_protocol(picture),
                None => image.empty_protocol(),
            }
        }
    }

    pub fn begin_search(&mut self, keyword: &str) {
        let selected = self.selected_path().unwrap_or_default().to_owned();
        self.remembered.insert(self.cwd.clone(), selected);
        self.search = Some(SearchView {
            root: self.cwd.clone(),
            keyword: keyword.to_string(),
            entries: Vec::new(),
            running: true,
        });
        self.visual = None;
        self.column.reset(&[], None);
    }

    pub fn add_search_results(&mut self, mut found: Vec<FileEntry>) {
        let sort = self.cache.sort();
        let Some(search) = self.search.as_mut() else {
            return;
        };
        let selected = self
            .column
            .selected(&search.entries)
            .map(|entry| entry.path.clone());
        search.entries.append(&mut found);
        sort_entries(&mut search.entries, sort);
        self.column.rebuild(&search.entries, selected.as_deref());
    }

    fn exit_search(&mut self) -> Cmd {
        self.search = None;
        self.visual = None;
        self.reset_column();
        Cmd::SelectionChanged
    }

    fn set_sort(&mut self, sort: SortMode) {
        let selected = self.selected_path().map(str::to_owned);
        self.cache.set_sort(sort);
        if let Some(search) = self.search.as_mut() {
            sort_entries(&mut search.entries, sort);
        }
        let entries = shown(self.search.as_ref(), &self.cache, &self.cwd);
        self.column.rebuild(entries, selected.as_deref());
    }

    fn moved(&mut self, moved: bool) -> Vec<Cmd> {
        if moved {
            self.cursor_moved_at = Instant::now();
            vec![Cmd::SelectionChanged]
        } else {
            Vec::new()
        }
    }

    pub fn preview_line_count(&self) -> usize {
        match &self.preview.body {
            PreviewBody::Text(text) => text.lines().count(),
            _ => 0,
        }
    }

    fn scroll_preview(&mut self, delta: isize) {
        let max = self
            .preview_line_count()
            .saturating_sub(usize::from(self.regions.preview.height).saturating_sub(6));
        self.preview.scroll = self.preview.scroll.saturating_add_signed(delta).min(max);
    }

    fn run(&mut self, operation: Operation) -> Vec<Cmd> {
        if operation.is_destructive() {
            self.overlay = Overlay::Confirm(operation);
            return Vec::new();
        }
        self.marks.clear();
        self.visual = None;
        vec![Cmd::Run(operation)]
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Cmd> {
        if self.input.is_some() {
            return self.handle_input(key);
        }
        if self.overlay != Overlay::None {
            return self.handle_overlay(key);
        }
        let Some(action) = action_for(key) else {
            self.pending = None;
            return Vec::new();
        };
        let pending = self.pending.take();
        if self.focus == Focus::Preview && self.scroll_preview_for(action) {
            return Vec::new();
        }
        let page = self.column.height().max(1) as isize;
        match action {
            Action::Down => self.cursor_by(1),
            Action::Up => self.cursor_by(-1),
            Action::HalfPageDown => self.cursor_by(page / 2),
            Action::HalfPageUp => self.cursor_by(-page / 2),
            Action::PageDown => self.cursor_by(page),
            Action::PageUp => self.cursor_by(-page),
            Action::Bottom => self.cursor_to(usize::MAX),
            Action::G if pending == Some(Action::G) || key.code == KeyCode::Home => {
                self.cursor_to(0)
            }
            Action::G | Action::D if pending != Some(action) => {
                self.pending = Some(action);
                Vec::new()
            }
            Action::D => {
                let paths = self.target_paths();
                if !paths.is_empty() {
                    self.overlay = Overlay::Confirm(Operation::Delete(paths));
                }
                Vec::new()
            }
            Action::Parent => self.leave(),
            Action::Open => self.open_selected(),
            Action::Mark => {
                if let Some(path) = self.selected_path().map(str::to_owned)
                    && !self.marks.remove(&path)
                {
                    self.marks.insert(path);
                }
                self.cursor_by(1)
            }
            Action::Visual => {
                if self.visual.is_some() {
                    self.commit_visual();
                } else if !self.column.is_empty() {
                    self.visual = Some(self.column.cursor());
                }
                Vec::new()
            }
            Action::MarkAll => self.toggle_mark_all(),
            Action::Filter => {
                let filter = self.column.filter().to_owned();
                self.begin_input(InputKind::Filter, &filter)
            }
            Action::Search => self.begin_input(InputKind::Search, ""),
            Action::Command => self.begin_input(InputKind::Command, ""),
            Action::Mkdir if self.search.is_none() => self.begin_input(InputKind::Mkdir, ""),
            Action::Rename => match self.selected().map(|entry| entry.server_filename.clone()) {
                Some(name) => self.begin_input(InputKind::Rename, &name),
                None => Vec::new(),
            },
            Action::Yank => self.copy_to_clipboard(false),
            Action::Cut => self.copy_to_clipboard(true),
            Action::Paste => self.paste(),
            Action::Download => self.download(),
            Action::Sort => {
                self.overlay = Overlay::Sort;
                Vec::new()
            }
            Action::Refresh => vec![Cmd::Refresh],
            Action::Tasks => {
                self.overlay = Overlay::Tasks { cursor: 0 };
                Vec::new()
            }
            Action::CancelTask => {
                let latest = self.tasks.active().next_back().map(|task| task.id);
                if let Some(id) = latest {
                    return vec![Cmd::Cancel(id)];
                }
                self.toast(ToastLevel::Info, "no running task");
                Vec::new()
            }
            Action::FocusNext => {
                self.focus = match self.focus {
                    Focus::Files if self.preview_line_count() > 0 => Focus::Preview,
                    _ => Focus::Files,
                };
                Vec::new()
            }
            Action::Help => {
                self.overlay = Overlay::Help;
                Vec::new()
            }
            Action::Escape => self.escape(),
            Action::Quit => {
                if self.tasks.has_active() {
                    self.overlay = Overlay::ConfirmQuit;
                } else {
                    self.quit = true;
                }
                Vec::new()
            }
            Action::G | Action::Mkdir => Vec::new(),
        }
    }

    /// Scroll the focused preview; returns whether `action` was a scroll.
    fn scroll_preview_for(&mut self, action: Action) -> bool {
        let page = (self.regions.preview.height / 2).max(1) as isize;
        let delta = match action {
            Action::Down => 1,
            Action::Up => -1,
            Action::HalfPageDown | Action::PageDown => page,
            Action::HalfPageUp | Action::PageUp => -page,
            _ => return false,
        };
        self.scroll_preview(delta);
        true
    }

    fn toggle_mark_all(&mut self) -> Vec<Cmd> {
        self.visual = None;
        let paths: Vec<String> = self
            .entries()
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        if paths.iter().all(|path| self.marks.contains(path)) {
            for path in &paths {
                self.marks.remove(path);
            }
        } else {
            self.marks.extend(paths);
        }
        Vec::new()
    }

    fn copy_to_clipboard(&mut self, cut: bool) -> Vec<Cmd> {
        let paths = self.target_paths();
        if !paths.is_empty() {
            let verb = if cut { "cut" } else { "yanked" };
            self.toast(
                ToastLevel::Info,
                format!("{verb} {} item(s) — p to paste", paths.len()),
            );
            self.clipboard = Some(Clipboard { paths, cut });
            self.marks.clear();
        }
        Vec::new()
    }

    fn download(&mut self) -> Vec<Cmd> {
        let files: Vec<FileEntry> = self
            .target_entries()
            .into_iter()
            .filter(|entry| !entry.is_dir && entry.fs_id != 0)
            .collect();
        self.marks.clear();
        if files.is_empty() {
            self.toast(ToastLevel::Info, "only files can be downloaded");
            return Vec::new();
        }
        vec![Cmd::Download(files)]
    }

    /// Back out one layer: selection, then filter, then search results.
    fn escape(&mut self) -> Vec<Cmd> {
        if self.visual.take().is_none() && self.marks.is_empty() && self.focus == Focus::Files {
            if !self.column.filter().is_empty() {
                let entries = shown(self.search.as_ref(), &self.cache, &self.cwd);
                self.column.set_filter("", entries);
            } else if self.search.is_some() {
                return vec![self.exit_search()];
            }
        }
        self.marks.clear();
        self.focus = Focus::Files;
        Vec::new()
    }

    fn cursor_by(&mut self, delta: isize) -> Vec<Cmd> {
        let moved = self.column.move_by(delta);
        self.moved(moved)
    }

    fn cursor_to(&mut self, position: usize) -> Vec<Cmd> {
        let moved = self.column.move_to(position);
        self.moved(moved)
    }

    fn leave(&mut self) -> Vec<Cmd> {
        if self.search.is_some() {
            return vec![self.exit_search()];
        }
        match remote_path::parent(&self.cwd) {
            Some(parent) => vec![self.enter_dir(&parent)],
            None => Vec::new(),
        }
    }

    /// Show `path` in its directory with the cursor on it.
    fn reveal(&mut self, path: &str) -> Vec<Cmd> {
        let Some(dir) = remote_path::parent(path) else {
            return Vec::new();
        };
        let cmd = self.enter_dir(&dir);
        self.remembered.insert(dir, path.to_string());
        self.column.reset(self.cache.entries(&self.cwd), Some(path));
        vec![cmd]
    }

    fn open_selected(&mut self) -> Vec<Cmd> {
        let Some(entry) = self.selected().cloned() else {
            return Vec::new();
        };
        if entry.is_dir {
            vec![self.enter_dir(&entry.path)]
        } else {
            vec![Cmd::Open(entry)]
        }
    }

    fn paste(&mut self) -> Vec<Cmd> {
        let Some(clipboard) = self.clipboard.as_ref() else {
            self.toast(ToastLevel::Info, "clipboard is empty — y to yank, x to cut");
            return Vec::new();
        };
        if self.search.is_some() {
            self.toast(ToastLevel::Info, "leave search results to paste");
            return Vec::new();
        }
        let cut = clipboard.cut;
        match ops::transfer_into(&clipboard.paths, &self.cwd, cut) {
            Ok(operation) => {
                if cut {
                    self.clipboard = None;
                }
                self.run(operation)
            }
            Err(error) => {
                self.toast(ToastLevel::Error, error);
                Vec::new()
            }
        }
    }

    fn begin_input(&mut self, kind: InputKind, text: &str) -> Vec<Cmd> {
        self.input = Some(Input {
            kind,
            edit: LineEdit::new(text),
        });
        Vec::new()
    }

    fn handle_input(&mut self, key: KeyEvent) -> Vec<Cmd> {
        let Some(input) = self.input.as_mut() else {
            return Vec::new();
        };
        let kind = input.kind;
        let edit = input.edit.handle(key);
        let text = input.edit.text().trim().to_string();
        let entries = shown(self.search.as_ref(), &self.cache, &self.cwd);
        match (edit, kind) {
            (Edit::Changed, InputKind::Filter) => {
                let before = self
                    .column
                    .selected(entries)
                    .map(|entry| entry.path.clone());
                self.column.set_filter(&text, entries);
                let after = self.column.selected(entries).map(|entry| &entry.path);
                return self.moved(before.as_ref() != after);
            }
            (Edit::Cancel, InputKind::Filter) => {
                self.input = None;
                self.column.set_filter("", entries);
                return vec![Cmd::SelectionChanged];
            }
            (Edit::Cancel, _) => self.input = None,
            (Edit::Submit, _) => {
                self.input = None;
                return self.submit(kind, &text);
            }
            _ => {}
        }
        Vec::new()
    }

    fn submit(&mut self, kind: InputKind, text: &str) -> Vec<Cmd> {
        if text.is_empty() {
            return Vec::new();
        }
        let result = match kind {
            InputKind::Filter => return Vec::new(),
            InputKind::Search => Ok(Command::Search {
                keyword: text.to_string(),
                mode: SearchMode::Recursive,
            }),
            InputKind::Mkdir => Ok(Command::Run(Operation::Mkdir(remote_path::resolve(
                &self.cwd, text,
            )))),
            InputKind::Rename => {
                let Some(from) = self.selected_path() else {
                    return Vec::new();
                };
                match ops::rename(from, text) {
                    Ok(Some(operation)) => Ok(Command::Run(operation)),
                    Ok(None) => return Vec::new(),
                    Err(error) => Err(error),
                }
            }
            InputKind::Command => {
                let targets = self.target_paths();
                ops::parse_command(
                    text,
                    &CommandContext {
                        cwd: &self.cwd,
                        targets,
                    },
                )
            }
        };
        match result {
            Ok(Command::Run(operation)) => self.run(operation),
            Ok(Command::Search { keyword, mode }) => {
                self.begin_search(&keyword);
                vec![Cmd::Search { keyword, mode }]
            }
            Err(error) => {
                self.toast(ToastLevel::Error, error);
                Vec::new()
            }
        }
    }

    fn handle_overlay(&mut self, key: KeyEvent) -> Vec<Cmd> {
        let close = matches!(key.code, KeyCode::Esc | KeyCode::Char('q'));
        match &mut self.overlay {
            Overlay::None => {}
            Overlay::Help => self.overlay = Overlay::None,
            Overlay::Sort => {
                if let KeyCode::Char(ch) = key.code
                    && let Some(sort_key) = SortKey::ALL.into_iter().find(|key| key.hotkey() == ch)
                {
                    let sort = self.cache.sort().choose(sort_key);
                    self.set_sort(sort);
                }
                self.overlay = Overlay::None;
            }
            Overlay::Tasks { cursor } => {
                let count = self.tasks.len();
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        *cursor = (*cursor + 1).min(count.saturating_sub(1))
                    }
                    KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                    KeyCode::Char('c' | 'x' | 'd') => {
                        let task = self
                            .tasks
                            .iter()
                            .rev()
                            .nth(*cursor)
                            .filter(|task| task.is_active());
                        if let Some(task) = task {
                            return vec![Cmd::Cancel(task.id)];
                        }
                    }
                    KeyCode::Char('T') => self.overlay = Overlay::None,
                    _ if close => self.overlay = Overlay::None,
                    _ => {}
                }
            }
            Overlay::Confirm(operation) => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                    let operation = operation.clone();
                    self.overlay = Overlay::None;
                    self.marks.clear();
                    self.visual = None;
                    return vec![Cmd::Run(operation)];
                }
                KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => {
                    self.overlay = Overlay::None;
                }
                _ => {}
            },
            Overlay::ConfirmQuit => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.quit = true,
                KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => {
                    self.overlay = Overlay::None;
                }
                _ => {}
            },
        }
        Vec::new()
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Vec<Cmd> {
        if self.input.is_some() || self.overlay != Overlay::None {
            return Vec::new();
        }
        let position = Position::new(mouse.column, mouse.row);
        let regions = &self.regions;
        let in_files = regions.files.contains(position);
        let in_preview = regions.preview.contains(position);
        let in_parent = regions.parent.contains(position);
        match mouse.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let delta = if mouse.kind == MouseEventKind::ScrollDown {
                    WHEEL_STEP
                } else {
                    -WHEEL_STEP
                };
                if in_files {
                    let moved = self.column.scroll_by(delta);
                    return self.moved(moved);
                }
                if in_preview {
                    self.scroll_preview(delta);
                }
                Vec::new()
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let double = self
                    .last_click
                    .is_some_and(|(at, last)| last == position && at.elapsed() < DOUBLE_CLICK);
                self.last_click = Some((Instant::now(), position));
                if let Some((_, path)) = self
                    .regions
                    .crumbs
                    .iter()
                    .find(|(area, _)| area.contains(position))
                {
                    let path = path.clone();
                    return vec![self.enter_dir(&path)];
                }
                if in_files {
                    self.focus = Focus::Files;
                    let Some(target) = self.file_row_at(position) else {
                        return Vec::new();
                    };
                    if double && target == self.column.cursor() {
                        return self.open_selected();
                    }
                    return self.cursor_to(target);
                }
                if in_parent {
                    return self.click_parent(position);
                }
                if in_preview {
                    if self.selected().is_some_and(|entry| entry.is_dir) {
                        return self.click_child(position);
                    }
                    if self.preview_line_count() > 0 {
                        self.focus = Focus::Preview;
                    }
                }
                Vec::new()
            }
            MouseEventKind::Down(MouseButton::Right) if in_files => {
                let Some(target) = self.file_row_at(position) else {
                    return Vec::new();
                };
                self.focus = Focus::Files;
                let mut cmds = self.cursor_to(target);
                cmds.extend(self.open_selected());
                cmds
            }
            _ => Vec::new(),
        }
    }

    /// Position in the file list of the row under `position`, if any.
    fn file_row_at(&self, position: Position) -> Option<usize> {
        let row = usize::from(position.y - self.regions.files.y);
        Some(self.column.offset() + row).filter(|&target| target < self.column.len())
    }

    /// Like yazi: a row of the parent column is revealed there (the cursor
    /// lands on it one level up); empty space just goes up.
    fn click_parent(&mut self, position: Position) -> Vec<Cmd> {
        let Some((dir, highlight)) = self.parent_column() else {
            return Vec::new();
        };
        match side_row(&self.cache, &dir, highlight, self.regions.parent, position) {
            Some(path) => self.reveal(&path),
            None => self.leave(),
        }
    }

    /// Like yazi: a row of the child column enters the hovered directory
    /// with the cursor on that row; empty space just enters.
    fn click_child(&mut self, position: Position) -> Vec<Cmd> {
        let Some(dir) = self.selected_path().map(str::to_owned) else {
            return Vec::new();
        };
        match side_row(&self.cache, &dir, None, self.regions.preview, position) {
            Some(path) => self.reveal(&path),
            None => vec![self.enter_dir(&dir)],
        }
    }
}

/// Entries shown in the middle column: search results, else the listing
/// of `cwd`. Borrows only what it reads, so callers may mutate other fields.
fn shown<'a>(
    search: Option<&'a SearchView>,
    cache: &'a DirectoryCache,
    cwd: &str,
) -> &'a [FileEntry] {
    match search {
        Some(search) => &search.entries,
        None => cache.entries(cwd),
    }
}

/// The entry under `position` in a side column listing `dir`.
fn side_row(
    cache: &DirectoryCache,
    dir: &str,
    highlight: Option<&str>,
    area: Rect,
    position: Position,
) -> Option<String> {
    let entries = cache.entries(dir);
    let (_, offset) = side_scroll(entries, highlight, usize::from(area.height));
    let row = offset + usize::from(position.y - area.y);
    entries.get(row).map(|entry| entry.path.clone())
}

/// Index of the `highlight`ed entry in a side column, and the scroll offset
/// it is drawn at. Shared by drawing and mouse hit-testing.
pub fn side_scroll(
    entries: &[FileEntry],
    highlight: Option<&str>,
    height: usize,
) -> (Option<usize>, usize) {
    let index = highlight.and_then(|path| entries.iter().position(|entry| entry.path == path));
    (index, side_offset(index, entries.len(), height))
}

/// Scroll offset that keeps `highlight` in the middle third of a side column.
fn side_offset(highlight: Option<usize>, len: usize, height: usize) -> usize {
    match highlight {
        Some(index) if index >= height.saturating_sub(height / 3) => (index + height / 3 + 1)
            .saturating_sub(height)
            .min(len.saturating_sub(height)),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::config::IconSet;

    fn entry(path: &str, is_dir: bool) -> FileEntry {
        FileEntry {
            fs_id: 1,
            path: path.to_string(),
            server_filename: remote_path::file_name(path).to_string(),
            size: 10,
            is_dir,
            ..FileEntry::default()
        }
    }

    fn app() -> App {
        let mut app = App::new("/docs", Theme::new(IconSet::Ascii));
        let generation = app.cache.begin_refresh("/docs");
        app.cache.finish_refresh(
            "/docs",
            generation,
            Ok(vec![
                entry("/docs/a.txt", false),
                entry("/docs/sub", true),
                entry("/docs/b.mkv", false),
            ]),
        );
        app.column.set_height(10);
        app.enter_dir("/docs");
        app
    }

    fn press(app: &mut App, keys: &str) -> Vec<Cmd> {
        keys.chars()
            .flat_map(|ch| app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)))
            .collect()
    }

    fn key(app: &mut App, code: KeyCode) -> Vec<Cmd> {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn directories_sort_first_and_navigation_round_trips() {
        let mut app = app();
        assert_eq!(app.selected().unwrap().path, "/docs/sub");
        assert_eq!(press(&mut app, "l"), [Cmd::Navigate]);
        assert_eq!(app.cwd, "/docs/sub");
        assert_eq!(press(&mut app, "h"), [Cmd::Navigate]);
        assert_eq!(app.selected().unwrap().path, "/docs/sub");
        assert_eq!(press(&mut app, "j"), [Cmd::SelectionChanged]);
        assert_eq!(
            press(&mut app, "l"),
            [Cmd::Open(entry("/docs/a.txt", false))]
        );
    }

    #[test]
    fn gg_and_dd_need_two_presses() {
        let mut app = app();
        press(&mut app, "G");
        assert_eq!(app.column.cursor(), 2);
        assert!(press(&mut app, "g").is_empty());
        assert_eq!(press(&mut app, "g"), [Cmd::SelectionChanged]);
        press(&mut app, "d");
        assert_eq!(app.overlay, Overlay::None);
        press(&mut app, "d");
        assert_eq!(
            app.overlay,
            Overlay::Confirm(Operation::Delete(vec!["/docs/sub".into()]))
        );
        assert_eq!(
            press(&mut app, "y"),
            [Cmd::Run(Operation::Delete(vec!["/docs/sub".into()]))]
        );
    }

    #[test]
    fn marks_and_visual_range_select_targets() {
        let mut app = app();
        press(&mut app, " ");
        assert!(app.marks.contains("/docs/sub"));
        press(&mut app, "v");
        press(&mut app, "j");
        assert_eq!(app.marked_count(), 3);
        let cmds = press(&mut app, "D");
        let Cmd::Download(files) = &cmds[0] else {
            panic!("expected download, got {cmds:?}");
        };
        assert_eq!(files.len(), 2);
        assert!(app.marks.is_empty());
    }

    #[test]
    fn filter_input_is_live_and_escape_clears() {
        let mut app = app();
        press(&mut app, "/mkv");
        assert_eq!(app.column.len(), 1);
        key(&mut app, KeyCode::Enter);
        assert!(app.input.is_none());
        assert_eq!(app.column.filter(), "mkv");
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.column.len(), 3);
    }

    #[test]
    fn yank_paste_creates_copy_operation() {
        let mut app = app();
        press(&mut app, "y");
        app.enter_dir("/other");
        let cmds = press(&mut app, "p");
        assert!(
            matches!(&cmds[..], [Cmd::Run(Operation::Copy(pairs))] if pairs[0].to == "/other/sub")
        );
    }

    #[test]
    fn command_line_reports_errors_as_toasts() {
        let mut app = app();
        press(&mut app, ":bogus");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].level, ToastLevel::Error);
    }

    #[test]
    fn search_mode_and_exit() {
        let mut app = app();
        press(&mut app, "sfoo");
        let cmds = key(&mut app, KeyCode::Enter);
        assert_eq!(
            cmds,
            [Cmd::Search {
                keyword: "foo".into(),
                mode: SearchMode::Recursive
            }]
        );
        app.add_search_results(vec![entry("/x/foo.txt", false)]);
        assert_eq!(app.entries().len(), 1);
        assert_eq!(key(&mut app, KeyCode::Esc), [Cmd::SelectionChanged]);
        assert_eq!(app.entries().len(), 3);
        assert_eq!(app.selected().unwrap().path, "/docs/sub");
    }

    #[test]
    fn side_offset_keeps_highlight_visible() {
        assert_eq!(side_offset(Some(2), 100, 10), 0);
        let offset = side_offset(Some(50), 100, 10);
        assert!((offset..offset + 10).contains(&50));
        assert_eq!(side_offset(Some(99), 100, 10), 90);
    }

    fn click(app: &mut App, column: u16, row: u16) -> Vec<Cmd> {
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    /// `/docs` listed under a loaded root, with a parent and child column.
    fn three_columns() -> App {
        let mut app = app();
        let generation = app.cache.begin_refresh("/");
        app.cache.finish_refresh(
            "/",
            generation,
            Ok(vec![entry("/docs", true), entry("/music", true)]),
        );
        let generation = app.cache.begin_refresh("/docs/sub");
        app.cache.finish_refresh(
            "/docs/sub",
            generation,
            Ok(vec![
                entry("/docs/sub/x.txt", false),
                entry("/docs/sub/y.txt", false),
            ]),
        );
        app.regions = Regions {
            parent: Rect::new(0, 1, 10, 5),
            files: Rect::new(11, 1, 30, 5),
            preview: Rect::new(42, 1, 30, 5),
            crumbs: Vec::new(),
        };
        app
    }

    #[test]
    fn clicking_parent_column_reveals_like_yazi() {
        let mut app = three_columns();
        assert_eq!(click(&mut app, 2, 1), [Cmd::Navigate]);
        assert_eq!(app.cwd, "/");
        assert_eq!(app.selected().unwrap().path, "/docs");

        let mut app = three_columns();
        click(&mut app, 2, 2);
        assert_eq!(app.cwd, "/");
        assert_eq!(app.selected().unwrap().path, "/music");

        let mut app = three_columns();
        click(&mut app, 2, 4);
        assert_eq!(app.cwd, "/");
        assert_eq!(app.selected().unwrap().path, "/docs");
    }

    #[test]
    fn clicking_child_column_enters_with_cursor_on_row() {
        let mut app = three_columns();
        assert_eq!(app.selected().unwrap().path, "/docs/sub");
        assert_eq!(click(&mut app, 45, 2), [Cmd::Navigate]);
        assert_eq!(app.cwd, "/docs/sub");
        assert_eq!(app.selected().unwrap().path, "/docs/sub/y.txt");

        let mut app = three_columns();
        click(&mut app, 45, 4);
        assert_eq!(app.cwd, "/docs/sub");
        assert_eq!(app.selected().unwrap().path, "/docs/sub/x.txt");
    }
}
