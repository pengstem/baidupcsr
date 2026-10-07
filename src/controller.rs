//! Runs the side effects the UI asks for ([`Cmd`]) and feeds results back
//! into it as [`Event`]s from background jobs.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use image::DynamicImage;
use ratatui_image::errors::Errors;
use ratatui_image::picker::Picker;
use ratatui_image::thread::{ResizeRequest, ResizeResponse, ThreadProtocol};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::app::directory::{DirectoryStore, Generation};
use crate::app::download::{DownloadOutcome, DownloadSpec, download};
use crate::app::fetch::{Account, RangeFetcher, RemoteFile};
use crate::app::launch;
use crate::app::ops::{self, Operation, SearchMode};
use crate::app::preview::{
    FileKind, PREVIEW_FETCH_BYTES, THUMBNAIL_MAX_BYTES, TextCache, ThumbnailCache, decode_image,
    describe_image,
};
use crate::app::remote_path;
use crate::app::task::{TaskId, TaskKind, TaskState};
use crate::config::{self, PcsConfig, UiConfig};
use crate::pcs::{ByteRange, FileEntry, PcsClient, PcsError, Quota};
use crate::tui::app::{App, Cmd, ImageState, PreviewBody, ToastLevel};
use crate::tui::theme::Theme;

/// Files up to this size can be opened with the desktop's default app.
const OPEN_LIMIT_BYTES: u64 = 100 * 1024 * 1024;
const TEXT_CACHE_BYTES: usize = 16 * 1024 * 1024;
const THUMBNAIL_CACHE_BYTES: usize = 32 * 1024 * 1024;

pub enum Event {
    Listed {
        path: String,
        generation: Generation,
        result: Result<Vec<FileEntry>, String>,
    },
    PreviewBytes {
        entry: FileEntry,
        result: Result<Bytes, String>,
    },
    Thumbnail {
        entry: FileEntry,
        /// Freshly downloaded bytes, to cache once they decode.
        fetched: Option<Bytes>,
        result: Result<DynamicImage, String>,
    },
    ImageEncoded(Result<ResizeResponse, Errors>),
    Quota(Quota),
    Progress {
        id: TaskId,
        done: u64,
    },
    Finished {
        id: TaskId,
        outcome: Outcome,
    },
    SearchBatch {
        id: TaskId,
        entries: Vec<FileEntry>,
    },
    Play {
        title: String,
        url: String,
    },
    Notice(ToastLevel, String),
}

pub enum Outcome {
    Done {
        message: String,
        refresh: Vec<String>,
        open: Option<PathBuf>,
    },
    Failed(String),
    Cancelled,
}

impl Outcome {
    fn done(message: String) -> Self {
        Self::Done {
            message,
            refresh: Vec::new(),
            open: None,
        }
    }
}

impl From<PcsError> for Outcome {
    fn from(error: PcsError) -> Self {
        if error.is_cancelled() {
            Self::Cancelled
        } else {
            Self::Failed(error.to_string())
        }
    }
}

struct Session {
    client: PcsClient,
    account: Option<Account>,
}

pub struct Controller {
    pub app: App,
    session: Option<Session>,
    pcs_config_path: PathBuf,
    store: DirectoryStore,
    ui: UiConfig,
    download_dir: PathBuf,
    texts: TextCache,
    thumbnails: ThumbnailCache,
    /// How images are drawn; known once the terminal has been queried.
    picker: Option<Picker>,
    resize_requests: Option<UnboundedReceiver<ResizeRequest>>,
    events: UnboundedSender<Event>,
    root: CancellationToken,
    jobs: JoinSet<()>,
    preview_cancel: CancellationToken,
    thumbnail_cancel: CancellationToken,
    task_cancels: HashMap<TaskId, CancellationToken>,
    search_task: Option<TaskId>,
    player: Option<tokio::process::Child>,
}

impl Controller {
    pub fn new(events: UnboundedSender<Event>) -> Self {
        let pcs_config_path = config::pcs_config_path(None);
        let (ui, ui_error) = match UiConfig::load(&config::ui_config_path()) {
            Ok(ui) => (ui, None),
            Err(error) => (UiConfig::default(), Some(error.to_string())),
        };
        let pcs = PcsConfig::load(&pcs_config_path);
        let user = pcs.as_ref().ok().and_then(PcsConfig::active_user);
        let workdir = user.map_or("/", |user| user.workdir.as_str());
        let mut app = App::new(workdir, Theme::new(ui.ui.icons));

        let session = match (&pcs, user) {
            (Err(error), _) => {
                app.toast(
                    ToastLevel::Error,
                    format!("{error} — log in with BaiduPCS-Go first"),
                );
                None
            }
            (Ok(_), None) => {
                app.toast(ToastLevel::Error, "no active account in BaiduPCS-Go config");
                None
            }
            (Ok(pcs), Some(user)) => match pcs.client(user, ui.network.proxy.clone()) {
                Ok(client) => Some(Session {
                    client,
                    account: user.account(),
                }),
                Err(error) => {
                    app.toast(ToastLevel::Error, error);
                    None
                }
            },
        };
        if let Some(error) = ui_error {
            app.toast(ToastLevel::Error, error);
        }
        let account_uid = user.map_or(0, |user| user.uid);
        let download_dir = ui.download_dir(pcs.as_ref().unwrap_or(&PcsConfig::default()));
        let root = CancellationToken::new();
        let (resize_tx, resize_requests) = mpsc::unbounded_channel();
        app.preview.image = Some(ThreadProtocol::new(resize_tx, None));
        Self {
            app,
            session,
            pcs_config_path,
            store: DirectoryStore::new(config::cache_dir(), account_uid),
            ui,
            download_dir,
            texts: TextCache::new(TEXT_CACHE_BYTES),
            thumbnails: ThumbnailCache::new(THUMBNAIL_CACHE_BYTES),
            picker: None,
            resize_requests: Some(resize_requests),
            events,
            preview_cancel: root.child_token(),
            thumbnail_cancel: root.child_token(),
            root,
            jobs: JoinSet::new(),
            task_cancels: HashMap::new(),
            search_task: None,
            player: None,
        }
    }

    pub fn set_picker(&mut self, picker: Picker) {
        self.picker = Some(picker);
    }

    pub fn start(&mut self) {
        if let Some(mut requests) = self.resize_requests.take() {
            // Resizing and encoding a picture can take a frame or more; only
            // the newest request matters, older ones belong to stale images.
            self.spawn(|events, cancel| async move {
                loop {
                    let mut request = tokio::select! {
                        () = cancel.cancelled() => break,
                        request = requests.recv() => match request {
                            Some(request) => request,
                            None => break,
                        },
                    };
                    while let Ok(newer) = requests.try_recv() {
                        request = newer;
                    }
                    if let Ok(result) =
                        tokio::task::spawn_blocking(move || request.resize_encode()).await
                    {
                        let _ = events.send(Event::ImageEncoded(result));
                    }
                }
            });
        }
        let cwd = self.app.cwd.clone();
        let cmd = self.app.enter_dir(&cwd);
        self.run(vec![cmd]);
        if let Some(session) = &self.session {
            let client = session.client.clone();
            self.spawn(|events, cancel| async move {
                if let Ok(quota) = client.quota(&cancel).await {
                    let _ = events.send(Event::Quota(quota));
                }
            });
        }
    }

    pub async fn shutdown(mut self) {
        self.root.cancel();
        if self.session.is_some()
            && let Err(error) = config::save_workdir(&self.pcs_config_path, &self.app.cwd)
        {
            eprintln!("could not save working directory: {error}");
        }
        let drained = tokio::time::timeout(Duration::from_secs(2), async {
            while self.jobs.join_next().await.is_some() {}
        });
        if drained.await.is_err() {
            self.jobs.abort_all();
        }
    }

    fn spawn<F, Fut>(&mut self, job: F)
    where
        F: FnOnce(UnboundedSender<Event>, CancellationToken) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.spawn_with(self.root.child_token(), job);
    }

    fn spawn_with<F, Fut>(&mut self, cancel: CancellationToken, job: F)
    where
        F: FnOnce(UnboundedSender<Event>, CancellationToken) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.jobs.spawn(job(self.events.clone(), cancel));
        while self.jobs.try_join_next().is_some() {}
    }

    /// Run `job` as a cancellable entry in the task list and report its
    /// outcome when it ends.
    fn spawn_task<F, Fut>(
        &mut self,
        kind: TaskKind,
        label: String,
        total: Option<u64>,
        job: F,
    ) -> TaskId
    where
        F: FnOnce(TaskId, UnboundedSender<Event>, CancellationToken) -> Fut,
        Fut: Future<Output = Outcome> + Send + 'static,
    {
        let id = self.app.tasks.start(kind, label, total);
        let cancel = self.root.child_token();
        self.task_cancels.insert(id, cancel.clone());
        self.spawn_with(cancel, |events, cancel| {
            let job = job(id, events.clone(), cancel);
            async move {
                let outcome = job.await;
                let _ = events.send(Event::Finished { id, outcome });
            }
        });
        id
    }

    fn client(&mut self) -> Option<PcsClient> {
        let client = self.session.as_ref().map(|session| session.client.clone());
        if client.is_none() {
            self.app.toast(ToastLevel::Error, "not logged in");
        }
        client
    }

    fn client_and_account(&mut self) -> Option<(PcsClient, Account)> {
        let client = self.client()?;
        let Some(account) = self.session.as_ref().and_then(|s| s.account.clone()) else {
            self.app
                .toast(ToastLevel::Error, "account has no uid/BDUSS for downloads");
            return None;
        };
        Some((client, account))
    }

    pub fn run(&mut self, cmds: Vec<Cmd>) {
        for cmd in cmds {
            match cmd {
                Cmd::Navigate => {
                    let cwd = self.app.cwd.clone();
                    self.ensure_listing(&cwd, false);
                    if let Some(parent) = remote_path::parent(&cwd) {
                        self.ensure_listing(&parent, false);
                    }
                    self.update_preview();
                }
                Cmd::SelectionChanged => self.update_preview(),
                Cmd::Refresh => {
                    let cwd = self.app.cwd.clone();
                    self.ensure_listing(&cwd, true);
                    if let Some(dir) = self.app.selected().filter(|entry| entry.is_dir) {
                        let dir = dir.path.clone();
                        self.ensure_listing(&dir, true);
                    }
                }
                Cmd::Open(entry) => self.open(entry),
                Cmd::Download(entries) => {
                    for entry in entries {
                        let target = self
                            .download_dir
                            .join(remote_path::local_file_name(&entry.server_filename));
                        self.start_download(TaskKind::Download, entry, target);
                    }
                }
                Cmd::Run(operation) => self.start_operation(operation),
                Cmd::Search { keyword, mode } => self.start_search(keyword, mode),
                Cmd::Cancel(id) => {
                    if self.app.tasks.request_cancel(id)
                        && let Some(cancel) = self.task_cancels.get(&id)
                    {
                        cancel.cancel();
                    }
                }
            }
        }
    }

    /// Show a listing from memory or disk, and fetch it if not fresh.
    fn ensure_listing(&mut self, path: &str, force: bool) {
        if !self
            .app
            .cache
            .get(path)
            .is_some_and(|listing| listing.is_loaded())
            && let Some((entries, saved_at)) = self.store.read(path)
        {
            self.app.cache.insert_stale(path, entries, saved_at);
            self.app.listing_updated(path);
        }
        if !force && !self.app.cache.needs_fetch(path) {
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        let client = session.client.clone();
        let generation = self.app.cache.begin_refresh(path);
        let path = path.to_string();
        self.spawn(|events, cancel| async move {
            let result = client.list(&path, &cancel).await;
            if !result.as_ref().is_err_and(|error| error.is_cancelled()) {
                let result = result.map_err(|error| error.to_string());
                let _ = events.send(Event::Listed {
                    path,
                    generation,
                    result,
                });
            }
        });
    }

    fn update_preview(&mut self) {
        self.preview_cancel.cancel();
        self.preview_cancel = self.root.child_token();
        if !self.app.is_selected(&self.app.preview.path) {
            self.thumbnail_cancel.cancel();
        }
        let Some(entry) = self.app.selected().cloned() else {
            self.app.set_preview("", PreviewBody::Empty);
            return;
        };
        if entry.is_dir {
            self.ensure_listing(&entry.path, false);
            self.app.set_preview(&entry.path, PreviewBody::Empty);
            return;
        }
        let kind = FileKind::of(&entry);
        let size = entry.size_bytes();
        let body = if kind == FileKind::Video {
            PreviewBody::Note("⏎  play in mpv      D  download".to_string())
        } else if !(kind.is_text() || kind == FileKind::Image) {
            PreviewBody::Note(if size <= OPEN_LIMIT_BYTES {
                "⏎  open with the default app      D  download".to_string()
            } else {
                "too large to open here      D  download".to_string()
            })
        } else if size == 0 {
            PreviewBody::Note("empty file".to_string())
        } else if let Some(text) = self.texts.get(&entry).filter(|_| kind.is_text()) {
            PreviewBody::Text(text)
        } else {
            match self
                .session
                .as_ref()
                .map(|session| (session.client.clone(), session.account.clone()))
            {
                Some((client, Some(account))) => {
                    let entry = entry.clone();
                    self.spawn_with(self.preview_cancel.clone(), |events, cancel| async move {
                        let file = RemoteFile::new(client, account, entry.path.clone());
                        let result = async {
                            file.fetch(ByteRange::chunk(0, PREVIEW_FETCH_BYTES, size)?, &cancel)
                                .await
                        }
                        .await;
                        if !result.as_ref().is_err_and(PcsError::is_cancelled) {
                            let _ = events.send(Event::PreviewBytes {
                                entry,
                                result: result.map_err(|error| error.to_string()),
                            });
                        }
                    });
                    PreviewBody::Loading
                }
                _ => PreviewBody::Note("preview needs a logged-in account".to_string()),
            }
        };
        self.app.set_preview(&entry.path, body);
        if kind == FileKind::Image && size > 0 {
            self.update_thumbnail(entry);
        }
    }

    /// Fetch and decode a server-side thumbnail of the selected image, unless
    /// one is already loading or shown.
    fn update_thumbnail(&mut self, entry: FileEntry) {
        if self.app.preview.image_state != ImageState::None {
            return;
        }
        let (Some(session), Some(picker)) = (&self.session, &self.picker) else {
            return;
        };
        let client = session.client.clone();
        let font = picker.font_size();
        let area = self.app.regions.preview;
        let pixels = (u32::from(area.width) * u32::from(font.width))
            .max(u32::from(area.height) * u32::from(font.height));
        let side = pixels.next_multiple_of(400).clamp(400, 1600);
        let cached = self.thumbnails.get(&entry);
        self.thumbnail_cancel.cancel();
        self.thumbnail_cancel = self.root.child_token();
        self.spawn_with(self.thumbnail_cancel.clone(), |events, cancel| async move {
            let (fetched, bytes) = match cached {
                Some(bytes) => (None, bytes),
                None => match client
                    .thumbnail(&entry.path, side, THUMBNAIL_MAX_BYTES, &cancel)
                    .await
                {
                    Ok(bytes) => (Some(bytes.clone()), bytes),
                    Err(error) if error.is_cancelled() => return,
                    Err(error) => {
                        let _ = events.send(Event::Thumbnail {
                            entry,
                            fetched: None,
                            result: Err(error.to_string()),
                        });
                        return;
                    }
                },
            };
            let result = tokio::task::spawn_blocking(move || decode_image(&bytes))
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
            if !cancel.is_cancelled() {
                let _ = events.send(Event::Thumbnail {
                    entry,
                    fetched,
                    result,
                });
            }
        });
        self.app.set_preview_image(ImageState::Loading, None);
    }

    fn open(&mut self, entry: FileEntry) {
        if FileKind::of(&entry) == FileKind::Video {
            self.play(entry);
            return;
        }
        if entry.size_bytes() > OPEN_LIMIT_BYTES {
            self.app
                .toast(ToastLevel::Info, "too large to open — press D to download");
            return;
        }
        let uid = self
            .session
            .as_ref()
            .and_then(|s| s.account.as_ref())
            .map_or(0, |a| a.uid);
        let target = std::env::temp_dir()
            .join("baidupcsr-open")
            .join(uid.to_string())
            .join(format!(
                "{}-{}-{}",
                entry.fs_id,
                entry.mtime,
                remote_path::local_file_name(&entry.server_filename)
            ));
        self.start_download(TaskKind::Open, entry, target);
    }

    fn play(&mut self, entry: FileEntry) {
        if !launch::program_available(&self.ui.player.command) {
            self.app.toast(
                ToastLevel::Error,
                format!(
                    "{} not found — install it or set [player] command",
                    self.ui.player.command
                ),
            );
            return;
        }
        let Some((client, account)) = self.client_and_account() else {
            return;
        };
        self.app.toast(
            ToastLevel::Info,
            format!("opening {} in player…", entry.server_filename),
        );
        self.spawn(|events, cancel| async move {
            let file = RemoteFile::new(client, account, entry.path);
            let event = match file.working_url(&cancel).await {
                Ok(url) => Event::Play {
                    title: entry.server_filename,
                    url,
                },
                Err(error) => Event::Notice(ToastLevel::Error, format!("cannot play: {error}")),
            };
            let _ = events.send(event);
        });
    }

    /// Download `entry` to `target`. A [`TaskKind::Open`] download reuses
    /// an earlier copy and opens the file once it is complete.
    fn start_download(&mut self, kind: TaskKind, entry: FileEntry, target: PathBuf) {
        let Some((client, account)) = self.client_and_account() else {
            return;
        };
        let open = kind == TaskKind::Open;
        let size = entry.size_bytes();
        let spec = DownloadSpec {
            target,
            size,
            chunk_size: self.ui.download.chunk_size,
            concurrency: self.ui.download.concurrency,
            reuse_existing: open,
        };
        let label = entry.server_filename.clone();
        self.spawn_task(kind, label, Some(size), |id, events, cancel| async move {
            let file = RemoteFile::new(client, account, entry.path);
            let progress = |done| {
                let _ = events.send(Event::Progress { id, done });
            };
            match download(&file, &spec, progress, &cancel).await {
                Ok(DownloadOutcome::Completed) if open => Outcome::Done {
                    message: format!("opened {}", entry.server_filename),
                    refresh: Vec::new(),
                    open: Some(spec.target),
                },
                Ok(DownloadOutcome::Completed) => {
                    Outcome::done(format!("saved {}", spec.target.display()))
                }
                Ok(DownloadOutcome::Cancelled { .. }) => Outcome::Cancelled,
                Err(error) => Outcome::Failed(error.to_string()),
            }
        });
    }

    fn start_operation(&mut self, operation: Operation) {
        let Some(client) = self.client() else {
            return;
        };
        let summary = operation.summary();
        self.spawn_task(
            TaskKind::Remote,
            summary.clone(),
            None,
            |_, _, cancel| async move {
                match operation.execute(&client, &cancel).await {
                    Ok(()) => Outcome::Done {
                        message: summary,
                        refresh: operation.affected_dirs(),
                        open: None,
                    },
                    Err(error) => error.into(),
                }
            },
        );
    }

    fn start_search(&mut self, keyword: String, mode: SearchMode) {
        if let Some(previous) = self.search_task.take() {
            self.run(vec![Cmd::Cancel(previous)]);
        }
        let Some(client) = self.client() else {
            return;
        };
        let root = self.app.cwd.clone();
        let label = format!("search “{keyword}”");
        let id = self.spawn_task(
            TaskKind::Search,
            label,
            None,
            |id, events, cancel| async move {
                let on_batch = |entries| {
                    let _ = events.send(Event::SearchBatch { id, entries });
                };
                match ops::search(&client, &root, &keyword, mode, on_batch, &cancel).await {
                    Ok(count) => Outcome::done(format!("{count} match(es) for “{keyword}”")),
                    Err(error) => error.into(),
                }
            },
        );
        self.search_task = Some(id);
    }

    pub fn handle(&mut self, event: Event) {
        while self.jobs.try_join_next().is_some() {}
        match event {
            Event::Listed {
                path,
                generation,
                result,
            } => {
                let failed = result.as_ref().err().cloned();
                let before = self.app.selected().cloned();
                if !self.app.cache.finish_refresh(&path, generation, result) {
                    return;
                }
                if failed.is_none() {
                    let (store, path) = (self.store.clone(), path.clone());
                    let entries = self.app.cache.entries(&path).to_vec();
                    self.jobs.spawn_blocking(move || {
                        let _ = store.write(&path, entries);
                    });
                }
                if let Some(error) = failed.filter(|_| path == self.app.cwd) {
                    self.app.toast(ToastLevel::Error, error);
                }
                // A refetch that leaves the selected entry as it was keeps its
                // preview; a failed one gets another try.
                if self.app.listing_updated(&path)
                    && (self.app.selected() != before.as_ref()
                        || matches!(self.app.preview.body, PreviewBody::Error(_)))
                {
                    self.update_preview();
                }
            }
            Event::PreviewBytes { entry, result } => {
                if !self.app.is_selected(&entry.path) {
                    return;
                }
                let body = match result {
                    Err(error) => PreviewBody::Error(error),
                    Ok(bytes) if FileKind::of(&entry).is_text() => {
                        let text = String::from_utf8_lossy(&bytes).into();
                        PreviewBody::Text(self.texts.insert(&entry, text))
                    }
                    Ok(bytes) => describe_image(&bytes).map_or_else(
                        || PreviewBody::Note("unrecognized image format".to_string()),
                        PreviewBody::Image,
                    ),
                };
                self.app.set_preview(&entry.path, body);
            }
            Event::Thumbnail {
                entry,
                fetched,
                result,
            } => {
                if let Some(bytes) = fetched
                    && result.is_ok()
                {
                    self.thumbnails.insert(&entry, bytes);
                }
                if !self.app.is_selected(&entry.path) {
                    return;
                }
                match (result, &self.picker) {
                    (Ok(image), Some(picker)) => self.app.set_preview_image(
                        ImageState::Ready,
                        Some(picker.new_resize_protocol(image)),
                    ),
                    (Ok(_), None) => {}
                    (Err(error), _) => self.app.set_preview_image(ImageState::Failed(error), None),
                }
            }
            Event::ImageEncoded(result) => match result {
                Ok(response) => {
                    if let Some(image) = &mut self.app.preview.image {
                        image.update_resized_protocol(response);
                    }
                }
                Err(error) => self
                    .app
                    .set_preview_image(ImageState::Failed(error.to_string()), None),
            },
            Event::Quota(quota) => self.app.quota = Some(quota),
            Event::Progress { id, done } => self.app.tasks.progress(id, done),
            Event::SearchBatch { id, entries } => {
                if self.search_task == Some(id) {
                    let before = self.app.selected_path().map(str::to_owned);
                    self.app.add_search_results(entries);
                    if self.app.selected_path() != before.as_deref() {
                        self.update_preview();
                    }
                }
            }
            Event::Finished { id, outcome } => self.finish(id, outcome),
            Event::Play { title, url } => {
                if let Some(mut previous) = self.player.take() {
                    let _ = previous.start_kill();
                }
                let user_agent = self
                    .session
                    .as_ref()
                    .map_or("netdisk", |session| session.client.pan_user_agent())
                    .to_string();
                match launch::spawn_player(&self.ui.player, &url, &user_agent, &title) {
                    Ok(child) => self.player = Some(child),
                    Err(error) => self
                        .app
                        .toast(ToastLevel::Error, format!("cannot start player: {error}")),
                }
            }
            Event::Notice(level, text) => self.app.toast(level, text),
        }
    }

    fn finish(&mut self, id: TaskId, outcome: Outcome) {
        self.task_cancels.remove(&id);
        if self.search_task == Some(id) {
            self.search_task = None;
            if let Some(search) = self.app.search.as_mut() {
                search.running = false;
            }
        }
        let label = self
            .app
            .tasks
            .get(id)
            .map(|task| task.label.clone())
            .unwrap_or_default();
        let state = match outcome {
            Outcome::Done {
                message,
                refresh,
                open,
            } => {
                if let Some(path) = open
                    && let Err(error) = launch::open_with_system(&path)
                {
                    self.app
                        .toast(ToastLevel::Error, format!("cannot open file: {error}"));
                }
                self.app.toast(ToastLevel::Success, message);
                self.refresh_dirs(&refresh);
                TaskState::Done
            }
            Outcome::Failed(error) => {
                self.app
                    .toast(ToastLevel::Error, format!("{label}: {error}"));
                TaskState::Failed(error)
            }
            Outcome::Cancelled => {
                self.app
                    .toast(ToastLevel::Info, format!("cancelled {label}"));
                TaskState::Cancelled
            }
        };
        self.app.tasks.finish(id, state);
    }

    /// Mark changed directories stale and refetch the ones on screen.
    fn refresh_dirs(&mut self, dirs: &[String]) {
        let visible = [
            Some(self.app.cwd.clone()),
            remote_path::parent(&self.app.cwd),
            self.app
                .selected()
                .filter(|entry| entry.is_dir)
                .map(|entry| entry.path.clone()),
        ];
        for dir in dirs {
            self.app.cache.invalidate(dir);
            if visible.iter().flatten().any(|shown| shown == dir) {
                self.ensure_listing(dir, true);
            }
        }
    }
}
