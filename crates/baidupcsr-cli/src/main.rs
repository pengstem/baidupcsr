use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const PREVIEW_MEMORY_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
const TEXT_PREVIEW_FETCH_BYTES: u64 = 64 * 1024;
const VIDEO_UPSTREAM_REDACTED_URL: &str = "https://pcs.baidu.com/rest/2.0/pcs/file";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_command(&args) {
        CommandAction::Doctor { config_dir } => {
            print_doctor(config_dir);
            ExitCode::SUCCESS
        }
        CommandAction::TuiShell => {
            let mut runtime = BrowserRuntime::from_config();
            let app = runtime.starting_app();
            match baidupcsr_tui::run_terminal_app_with_handler(app, move |app, event| {
                runtime.handle_event(app, event);
            }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("tui_error: {err}");
                    ExitCode::from(1)
                }
            }
        }
        CommandAction::UsageError => {
            eprintln!("usage: baidupcsr-cli [tui|doctor [--redact]]");
            ExitCode::from(2)
        }
    }
}

struct BrowserRuntime {
    client: Option<baidupcsr_pcs::BlockingPcsClient>,
    account_uid: u64,
    account_bduss: Option<String>,
    cache: baidupcsr_core::DirectoryCache,
    loaded_at: HashMap<String, Instant>,
    selection_memory: HashMap<String, String>,
    preview_manager: baidupcsr_preview::PreviewManager,
    preview_epoch: Arc<AtomicU64>,
    video_config: baidupcsr_config::VideoPreviewConfig,
    video_epoch: Arc<AtomicU64>,
    active_video: Option<ActiveVideoPreview>,
    tx: mpsc::Sender<DirectoryLoadResult>,
    rx: mpsc::Receiver<DirectoryLoadResult>,
    preview_tx: mpsc::Sender<PreviewLoadResult>,
    preview_rx: mpsc::Receiver<PreviewLoadResult>,
    inflight: usize,
    initial_path: String,
    startup_notice: Option<String>,
}

struct DirectoryLoadResult {
    generation: baidupcsr_core::RefreshGeneration,
    path: String,
    result: Result<Vec<baidupcsr_pcs::FileEntry>, String>,
}

struct PreviewLoadResult {
    request: baidupcsr_preview::PreviewRequest,
    title: String,
    result: Result<Vec<u8>, String>,
}

struct ActiveVideoPreview {
    key: baidupcsr_preview::PreviewKey,
    title: String,
    server: baidupcsr_preview::VideoLoopbackServer,
    player: Child,
}

impl Drop for ActiveVideoPreview {
    fn drop(&mut self) {
        let _ = self.player.kill();
        let _ = self.player.wait();
        self.server.shutdown();
    }
}

impl BrowserRuntime {
    fn from_config() -> Self {
        let (tx, rx) = mpsc::channel();
        let (preview_tx, preview_rx) = mpsc::channel();
        let ui_config_path = baidupcsr_config::discover_ui_config_file();
        let ui_config = baidupcsr_config::load_ui_config_or_default(&ui_config_path)
            .unwrap_or_else(|_| baidupcsr_config::UiConfig::default());
        let discovery = baidupcsr_config::discovery_input_from_env(None);
        let config_path = baidupcsr_config::discover_config_file(&discovery);
        let config = match baidupcsr_config::load_config(&config_path) {
            Ok(config) => config,
            Err(err) => {
                return Self {
                    client: None,
                    account_uid: 0,
                    account_bduss: None,
                    cache: baidupcsr_core::DirectoryCache::new(),
                    loaded_at: HashMap::new(),
                    selection_memory: HashMap::new(),
                    preview_manager: baidupcsr_preview::PreviewManager::new(
                        PREVIEW_MEMORY_LIMIT_BYTES,
                    ),
                    preview_epoch: Arc::new(AtomicU64::new(0)),
                    video_config: ui_config.video_preview,
                    video_epoch: Arc::new(AtomicU64::new(0)),
                    active_video: None,
                    tx,
                    rx,
                    preview_tx,
                    preview_rx,
                    inflight: 0,
                    initial_path: "/".to_string(),
                    startup_notice: Some(format!("config_error: {err}")),
                };
            }
        };

        let Some(user) = baidupcsr_config::active_user(&config) else {
            return Self {
                client: None,
                account_uid: 0,
                account_bduss: None,
                cache: baidupcsr_core::DirectoryCache::new(),
                loaded_at: HashMap::new(),
                selection_memory: HashMap::new(),
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
                video_config: ui_config.video_preview,
                video_epoch: Arc::new(AtomicU64::new(0)),
                active_video: None,
                tx,
                rx,
                preview_tx,
                preview_rx,
                inflight: 0,
                initial_path: "/".to_string(),
                startup_notice: Some("active_user: missing".to_string()),
            };
        };

        let initial_path = if user.workdir.is_empty() {
            "/".to_string()
        } else {
            user.workdir.to_string()
        };

        let client =
            build_pcs_client(&config, user).map_err(|message| format!("auth_error: {message}"));
        let account_bduss = bduss_from_user(user);

        match client {
            Ok(client) => Self {
                client: Some(client),
                account_uid: user.uid,
                account_bduss,
                cache: baidupcsr_core::DirectoryCache::new(),
                loaded_at: HashMap::new(),
                selection_memory: HashMap::new(),
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
                video_config: ui_config.video_preview,
                video_epoch: Arc::new(AtomicU64::new(0)),
                active_video: None,
                tx,
                rx,
                preview_tx,
                preview_rx,
                inflight: 0,
                initial_path,
                startup_notice: None,
            },
            Err(message) => Self {
                client: None,
                account_uid: user.uid,
                account_bduss,
                cache: baidupcsr_core::DirectoryCache::new(),
                loaded_at: HashMap::new(),
                selection_memory: HashMap::new(),
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
                video_config: ui_config.video_preview,
                video_epoch: Arc::new(AtomicU64::new(0)),
                active_video: None,
                tx,
                rx,
                preview_tx,
                preview_rx,
                inflight: 0,
                initial_path,
                startup_notice: Some(message),
            },
        }
    }

    fn starting_app(&mut self) -> baidupcsr_tui::TuiApp {
        let mut app = baidupcsr_tui::TuiApp::new(&self.initial_path);
        if let Some(notice) = self.startup_notice.clone() {
            app.set_notice(notice);
        } else {
            self.start_directory_load(&mut app, self.initial_path.clone(), None);
        }
        app
    }

    fn handle_event(
        &mut self,
        app: &mut baidupcsr_tui::TuiApp,
        event: baidupcsr_tui::TuiRuntimeEvent,
    ) {
        match event {
            baidupcsr_tui::TuiRuntimeEvent::Tick => {
                self.drain_directory_results(app);
                self.drain_preview_results(app);
                self.refresh_active_video_preview(app);
                self.refresh_current_cache_age(app);
            }
            baidupcsr_tui::TuiRuntimeEvent::Action(action) => self.handle_action(app, action),
        }
    }

    fn handle_action(&mut self, app: &mut baidupcsr_tui::TuiApp, action: baidupcsr_tui::KeyAction) {
        match action {
            baidupcsr_tui::KeyAction::Enter => {
                if let Some(entry) = app.selected_entry().cloned() {
                    if entry.is_dir {
                        self.stop_active_video();
                        self.selection_memory
                            .insert(app.path().to_string(), entry.path.clone());
                        self.start_directory_load(app, entry.path, None);
                    } else {
                        app.set_notice(format!(
                            "preview_pending: {} is a file; l does not enter",
                            entry.server_filename
                        ));
                    }
                }
            }
            baidupcsr_tui::KeyAction::Back => {
                if let Some(path) = baidupcsr_tui::TuiApp::parent_path(app.path()) {
                    self.stop_active_video();
                    self.start_directory_load(app, path, Some(app.path().to_string()));
                }
            }
            baidupcsr_tui::KeyAction::Refresh => {
                let preferred = app.selected_entry().map(|entry| entry.path.clone());
                self.start_directory_load(app, app.path().to_string(), preferred);
            }
            baidupcsr_tui::KeyAction::Up | baidupcsr_tui::KeyAction::Down => {
                self.prefetch_selected_directory(app);
                self.update_preview_for_selection(app);
            }
            baidupcsr_tui::KeyAction::Preview => {
                self.start_video_preview(app);
            }
            baidupcsr_tui::KeyAction::Search
            | baidupcsr_tui::KeyAction::Command
            | baidupcsr_tui::KeyAction::Quit
            | baidupcsr_tui::KeyAction::None => {}
        }
    }

    fn start_directory_load(
        &mut self,
        app: &mut baidupcsr_tui::TuiApp,
        path: String,
        preferred_selection_path: Option<String>,
    ) {
        self.stop_active_video();
        let view = self.cache.enter_directory(path.as_str());
        let cache_age = self.cache_age_seconds(path.as_str());
        let preferred = preferred_selection_path
            .as_deref()
            .or_else(|| self.selection_memory.get(path.as_str()).map(String::as_str));
        app.begin_directory_refresh(path.as_str(), view.entries, cache_age, preferred);
        app.begin_preview(path.as_str(), "directory loading");

        let Some(client) = self.client.clone() else {
            let view = self.cache.snapshot(path.as_str());
            app.finish_directory_error(
                path.as_str(),
                view.entries,
                cache_age,
                "remote_list_error: client unavailable",
                preferred,
            );
            return;
        };

        self.spawn_directory_load(client, path, view.generation);
        app.set_background_tasks(self.inflight);
    }

    fn prefetch_selected_directory(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        let Some(path) = app.selected_directory_path() else {
            return;
        };
        let Some(client) = self.client.clone() else {
            return;
        };
        if self.cache.snapshot(path.as_str()).status == baidupcsr_core::DirectoryStatus::Refreshing
        {
            return;
        }

        let generation = self.cache.begin_refresh(path.as_str());
        self.spawn_directory_load(client, path, generation);
        app.set_background_tasks(self.inflight);
    }

    fn spawn_directory_load(
        &mut self,
        client: baidupcsr_pcs::BlockingPcsClient,
        path: String,
        generation: baidupcsr_core::RefreshGeneration,
    ) {
        self.inflight = self.inflight.saturating_add(1);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = list_remote_directory(&client, path.as_str());
            let _ = tx.send(DirectoryLoadResult {
                generation,
                path,
                result,
            });
        });
    }

    fn drain_directory_results(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        while let Ok(result) = self.rx.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            let path = result.path;
            match result.result {
                Ok(entries) => {
                    let refresh =
                        self.cache
                            .finish_refresh(path.as_str(), result.generation, entries);
                    if refresh.applied {
                        self.loaded_at.insert(path.clone(), Instant::now());
                        if app.path() == path {
                            let view = self.cache.snapshot(path.as_str());
                            let preferred =
                                self.selection_memory.get(path.as_str()).map(String::as_str);
                            app.finish_directory_success(
                                path.as_str(),
                                view.entries,
                                self.cache_age_seconds(path.as_str()),
                                preferred,
                            );
                            self.update_preview_for_selection(app);
                        }
                    }
                }
                Err(message) => {
                    let refresh = self
                        .cache
                        .finish_refresh_error(path.as_str(), result.generation);
                    if refresh.applied && app.path() == path {
                        let view = self.cache.snapshot(path.as_str());
                        let preferred =
                            self.selection_memory.get(path.as_str()).map(String::as_str);
                        app.finish_directory_error(
                            path.as_str(),
                            view.entries,
                            self.cache_age_seconds(path.as_str()),
                            message,
                            preferred,
                        );
                    }
                }
            }
        }
        app.set_background_tasks(self.inflight);
    }

    fn update_preview_for_selection(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        let preview_epoch = self
            .preview_epoch
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.stop_active_video_if_selection_changed(app);
        let Some(entry) = app.selected_entry().cloned() else {
            app.begin_preview("metadata", "no selected entry");
            return;
        };

        let size = nonnegative_size(entry.size);
        let plan = baidupcsr_preview::PreviewPlan::for_entry(
            entry.server_filename.as_str(),
            entry.is_dir,
            size,
            PREVIEW_MEMORY_LIMIT_BYTES,
        );
        let budget = baidupcsr_tui::PreviewBudget {
            kind: tui_preview_kind(plan.kind),
            fetched_bytes: 0,
            memory_limit_bytes: PREVIEW_MEMORY_LIMIT_BYTES,
            truncated: false,
            disk_backed: false,
        };

        if plan.kind == baidupcsr_preview::PreviewKind::Video {
            let reason = if self.video_config.enabled {
                "press p for strict loopback video preview"
            } else {
                "video preview disabled by sidecar kill switch"
            };
            app.finish_preview_metadata(
                entry.server_filename.as_str(),
                baidupcsr_tui::PreviewKind::Video,
                reason,
                baidupcsr_tui::PreviewBudget {
                    kind: baidupcsr_tui::PreviewKind::Video,
                    ..budget
                },
            );
            return;
        }

        if plan.action == baidupcsr_preview::PreviewAction::MetadataOnly {
            app.finish_preview_metadata(
                entry.server_filename.as_str(),
                tui_preview_kind(plan.kind),
                plan.reason,
                budget,
            );
            return;
        }

        let key = preview_key_for_entry(self.account_uid, &entry);
        if let Some(text) = self.preview_manager.get(&key) {
            app.finish_preview_text(
                entry.server_filename.as_str(),
                text.content.as_str(),
                preview_budget_from_status(text.status),
            );
            return;
        }

        let Some(client) = self.client.clone() else {
            app.finish_preview_error(entry.server_filename.as_str(), "client unavailable");
            return;
        };

        let request = self.preview_manager.start_request(key);
        app.begin_preview(entry.server_filename.as_str(), "loading text preview");
        let fetch_bytes = size.min(TEXT_PREVIEW_FETCH_BYTES);
        if fetch_bytes == 0 {
            let update = self.preview_manager.finish_text_request(request, b"");
            if update.request_state == baidupcsr_preview::RequestState::Applied {
                app.finish_preview_text(
                    entry.server_filename.as_str(),
                    "",
                    preview_budget_from_status(update.status),
                );
            }
            return;
        }

        self.spawn_preview_load(
            client,
            entry.path,
            entry.server_filename,
            request,
            fetch_bytes,
            preview_epoch,
        );
        app.set_background_tasks(self.inflight);
    }

    fn spawn_preview_load(
        &mut self,
        client: baidupcsr_pcs::BlockingPcsClient,
        path: String,
        title: String,
        request: baidupcsr_preview::PreviewRequest,
        fetch_bytes: u64,
        preview_epoch: u64,
    ) {
        self.inflight = self.inflight.saturating_add(1);
        let tx = self.preview_tx.clone();
        let cancellation_epoch = Arc::clone(&self.preview_epoch);
        thread::spawn(move || {
            let range = baidupcsr_pcs::ByteRange::prefix(fetch_bytes);
            let result = client
                .download_range_with_cancel(path.as_str(), range, || {
                    cancellation_epoch.load(Ordering::Acquire) != preview_epoch
                })
                .map_err(|err| format!("preview_error: {err}"));
            let _ = tx.send(PreviewLoadResult {
                request,
                title,
                result,
            });
        });
    }

    fn drain_preview_results(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        while let Ok(result) = self.preview_rx.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            match result.result {
                Ok(bytes) => {
                    let key = result.request.key();
                    let update = self
                        .preview_manager
                        .finish_text_request(result.request, &bytes);
                    if update.request_state == baidupcsr_preview::RequestState::Applied
                        && self.selected_preview_key(app) == Some(key)
                    {
                        if let Some(text) = self.preview_manager.get(&key) {
                            app.finish_preview_text(
                                result.title.as_str(),
                                text.content.as_str(),
                                preview_budget_from_status(text.status),
                            );
                        }
                    }
                }
                Err(message) => {
                    if self.selected_preview_key(app) == Some(result.request.key()) {
                        app.finish_preview_error(result.title.as_str(), message);
                    }
                }
            }
        }
        app.set_background_tasks(self.inflight);
    }

    fn start_video_preview(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        let Some(entry) = app.selected_entry().cloned() else {
            app.set_notice("video_preview: no selected entry");
            return;
        };
        if !is_video_entry(&entry) {
            app.set_notice(format!(
                "video_preview: {} is not a supported video file",
                entry.server_filename
            ));
            return;
        }
        if !self.video_config.enabled {
            app.finish_preview_metadata(
                entry.server_filename.as_str(),
                baidupcsr_tui::PreviewKind::Video,
                "video preview disabled by sidecar kill switch",
                baidupcsr_tui::PreviewBudget {
                    kind: baidupcsr_tui::PreviewKind::Video,
                    fetched_bytes: 0,
                    memory_limit_bytes: video_memory_limit_from_config(&self.video_config),
                    truncated: false,
                    disk_backed: false,
                },
            );
            return;
        }

        let Some(client) = self.client.clone() else {
            app.finish_preview_error(entry.server_filename.as_str(), "client unavailable");
            return;
        };
        let Some(account_bduss) = self.account_bduss.clone() else {
            app.finish_preview_error(
                entry.server_filename.as_str(),
                "video_auth_error: missing BDUSS",
            );
            return;
        };
        if self.account_uid == 0 {
            app.finish_preview_error(
                entry.server_filename.as_str(),
                "video_auth_error: missing uid",
            );
            return;
        }
        let budget = match video_budget_from_config(&self.video_config) {
            Ok(budget) => budget,
            Err(err) => {
                app.finish_preview_error(
                    entry.server_filename.as_str(),
                    format!("video_budget_error: {err:?}"),
                );
                return;
            }
        };
        self.stop_active_video();
        let token_epoch = self
            .video_epoch
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let token = match generate_video_token(token_epoch) {
            Ok(token) => token,
            Err(err) => {
                app.finish_preview_error(
                    entry.server_filename.as_str(),
                    format!("video_token_error: {err:?}"),
                );
                return;
            }
        };
        let key = preview_key_for_entry(self.account_uid, &entry);
        let path = entry.path.clone();
        let title = entry.server_filename.clone();
        let account_uid = self.account_uid;
        let cancellation_epoch = Arc::clone(&self.video_epoch);
        let dlink_cache = Arc::new(Mutex::new(None::<String>));
        let server = baidupcsr_preview::VideoLoopbackServer::spawn(
            key,
            match baidupcsr_preview::VideoUpstream::new(VIDEO_UPSTREAM_REDACTED_URL) {
                Ok(upstream) => upstream,
                Err(err) => {
                    app.finish_preview_error(
                        entry.server_filename.as_str(),
                        format!("video_upstream_error: {err:?}"),
                    );
                    return;
                }
            },
            token,
            budget,
            move |range| {
                if cancellation_epoch.load(Ordering::Acquire) != token_epoch {
                    return Err(baidupcsr_preview::VideoPreviewError::InactiveVideoSession);
                }
                let byte_range = baidupcsr_pcs::ByteRange::new(range.start, range.end_inclusive)
                    .map_err(|_| baidupcsr_preview::VideoPreviewError::InvalidRange)?;
                let dlink = {
                    let mut cached = dlink_cache
                        .lock()
                        .map_err(|_| baidupcsr_preview::VideoPreviewError::LoopbackServerIo)?;
                    if let Some(dlink) = cached.as_ref() {
                        dlink.clone()
                    } else {
                        let urls = client
                            .locate_download_urls(
                                path.as_str(),
                                account_uid,
                                account_bduss.as_str(),
                            )
                            .map_err(|_| baidupcsr_preview::VideoPreviewError::LoopbackServerIo)?;
                        let dlink = urls
                            .into_iter()
                            .next()
                            .ok_or(baidupcsr_preview::VideoPreviewError::LoopbackServerIo)?;
                        *cached = Some(dlink.clone());
                        dlink
                    }
                };
                client
                    .download_url_range_with_cancel(dlink.as_str(), byte_range, || {
                        cancellation_epoch.load(Ordering::Acquire) != token_epoch
                    })
                    .map_err(|_| baidupcsr_preview::VideoPreviewError::LoopbackServerIo)
            },
        );
        let server = match server {
            Ok(server) => server,
            Err(err) => {
                app.finish_preview_error(
                    entry.server_filename.as_str(),
                    format!("video_server_error: {err:?}"),
                );
                return;
            }
        };
        let launch = match server.player_launch(baidupcsr_preview::PlayerBackend::Mpv) {
            Ok(launch) => launch,
            Err(err) => {
                app.finish_preview_error(
                    entry.server_filename.as_str(),
                    format!("video_player_error: {err:?}"),
                );
                return;
            }
        };
        let child = Command::new(launch.program())
            .args(launch.args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(err) => {
                app.finish_preview_error(
                    entry.server_filename.as_str(),
                    format!("video_player_spawn_error: {err}"),
                );
                return;
            }
        };
        self.active_video = Some(ActiveVideoPreview {
            key,
            title,
            server,
            player: child,
        });
        self.refresh_active_video_preview(app);
    }

    fn refresh_active_video_preview(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        if self.active_video.is_none() {
            return;
        }
        self.stop_active_video_if_selection_changed(app);
        let Some(active) = self.active_video.as_mut() else {
            return;
        };
        let exited = match active.player.try_wait() {
            Ok(Some(status)) => Some(format!("player_exit={status}")),
            Ok(None) => None,
            Err(err) => Some(format!("player_poll_error={err}")),
        };
        let stats = active.server.stats();
        let memory_limit = video_memory_limit_from_config(&self.video_config);
        let player_line =
            exited.unwrap_or_else(|| format!("player=mpv pid={}", active.player.id()));
        let lines = vec![
            "video=loopback seek=true".to_string(),
            format!("listen=127.0.0.1:{}", active.server.endpoint().port()),
            player_line,
            format!(
                "range={} window={}",
                video_range_label(stats.last_range_start, stats.last_range_end_inclusive),
                video_range_label(stats.window_start, stats.window_end_inclusive)
            ),
            format!(
                "cache={}/{} hits={} upstream={}",
                stats.used_bytes, memory_limit, stats.cache_hits, stats.upstream_fetches
            ),
            format!(
                "disk=false error={}",
                video_error_label(stats.last_error.as_ref())
            ),
        ];
        app.finish_preview_video(
            active.title.as_str(),
            lines,
            baidupcsr_tui::PreviewBudget {
                kind: baidupcsr_tui::PreviewKind::Video,
                fetched_bytes: stats.used_bytes,
                memory_limit_bytes: memory_limit,
                truncated: false,
                disk_backed: false,
            },
        );
    }

    fn stop_active_video_if_selection_changed(&mut self, app: &baidupcsr_tui::TuiApp) {
        let active_key = self.active_video.as_ref().map(|active| active.key);
        if active_key.is_some() && self.selected_preview_key(app) != active_key {
            self.stop_active_video();
        }
    }

    fn stop_active_video(&mut self) {
        self.video_epoch.fetch_add(1, Ordering::AcqRel);
        self.active_video = None;
    }

    fn selected_preview_key(
        &self,
        app: &baidupcsr_tui::TuiApp,
    ) -> Option<baidupcsr_preview::PreviewKey> {
        app.selected_entry()
            .map(|entry| preview_key_for_entry(self.account_uid, entry))
    }

    fn refresh_current_cache_age(&self, app: &mut baidupcsr_tui::TuiApp) {
        app.set_cache_age_seconds(self.cache_age_seconds(app.path()));
    }

    fn cache_age_seconds(&self, path: &str) -> Option<u64> {
        self.loaded_at
            .get(path)
            .map(|loaded_at| loaded_at.elapsed().as_secs())
    }
}

enum CommandAction {
    Doctor { config_dir: Option<PathBuf> },
    TuiShell,
    UsageError,
}

fn parse_command(args: &[String]) -> CommandAction {
    match args {
        [] => CommandAction::TuiShell,
        [cmd] if cmd == "tui" => CommandAction::TuiShell,
        [cmd] if cmd == "doctor" => CommandAction::Doctor { config_dir: None },
        [cmd, flag] if cmd == "doctor" && flag == "--redact" => {
            CommandAction::Doctor { config_dir: None }
        }
        [cmd, flag, dir_flag, dir]
            if cmd == "doctor" && flag == "--redact" && dir_flag == "--config-dir" =>
        {
            CommandAction::Doctor {
                config_dir: Some(PathBuf::from(dir)),
            }
        }
        _ => CommandAction::UsageError,
    }
}

fn build_pcs_client(
    config: &baidupcsr_config::PcsConfig,
    user: &baidupcsr_config::BaiduUser,
) -> Result<baidupcsr_pcs::BlockingPcsClient, String> {
    let auth = if !user.cookies.is_empty() {
        Some(baidupcsr_pcs::PcsAuth::from_cookie_header(
            user.cookies.as_str(),
        ))
    } else {
        baidupcsr_pcs::PcsAuth::from_bduss_stoken(
            user.bduss.as_str(),
            Some(user.stoken.as_str()),
            Some(user.baiduid.as_str()),
        )
    }
    .ok_or_else(|| "missing BDUSS/cookies in active user".to_string())?;

    let pcs_addr = if config.pcs_addr.is_empty() {
        "pcs.baidu.com"
    } else {
        config.pcs_addr.as_str()
    };
    let user_agent = pcs_user_agent_for_config(config);
    let pan_user_agent = pan_user_agent_for_config(config);

    let api = baidupcsr_pcs::PcsApi::new(baidupcsr_pcs::PcsClientOptions {
        app_id: config.appid,
        pcs_addr: pcs_addr.to_string(),
        enable_https: config.enable_https,
    });
    Ok(baidupcsr_pcs::BlockingPcsClient::new(
        api,
        auth,
        baidupcsr_pcs::BlockingPcsClientOptions::new_with_pan(
            user_agent,
            pan_user_agent,
            optional_string(config.proxy.as_str()),
        ),
    ))
}

fn list_remote_directory(
    client: &baidupcsr_pcs::BlockingPcsClient,
    path: &str,
) -> Result<Vec<baidupcsr_pcs::FileEntry>, String> {
    client
        .list(
            path,
            baidupcsr_pcs::OrderOptions {
                by: baidupcsr_pcs::OrderBy::Name,
                direction: baidupcsr_pcs::OrderDirection::Asc,
            },
        )
        .map_err(|err| format!("remote_list_error: {err}"))
}

fn is_video_entry(entry: &baidupcsr_pcs::FileEntry) -> bool {
    !entry.is_dir
        && baidupcsr_preview::preview_kind_from_name(entry.server_filename.as_str())
            == baidupcsr_preview::PreviewKind::Video
}

fn video_budget_from_config(
    config: &baidupcsr_config::VideoPreviewConfig,
) -> Result<baidupcsr_preview::VideoCacheBudget, baidupcsr_preview::VideoPreviewError> {
    baidupcsr_preview::VideoCacheBudget::new(config.chunk_size_bytes, config.max_chunks)
}

fn video_memory_limit_from_config(config: &baidupcsr_config::VideoPreviewConfig) -> u64 {
    config
        .chunk_size_bytes
        .saturating_mul(u64::try_from(config.max_chunks).unwrap_or(u64::MAX))
}

fn video_range_label(start: Option<u64>, end_inclusive: Option<u64>) -> String {
    match (start, end_inclusive) {
        (Some(start), Some(end_inclusive)) => format!("{start}-{end_inclusive}"),
        _ => "empty".to_string(),
    }
}

fn video_error_label(error: Option<&baidupcsr_preview::VideoPreviewError>) -> String {
    error
        .map(|error| format!("{error:?}"))
        .unwrap_or_else(|| "none".to_string())
}

fn generate_video_token(
    epoch: u64,
) -> Result<baidupcsr_preview::VideoToken, baidupcsr_preview::VideoPreviewError> {
    let mut bytes = [0_u8; 16];
    if File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_err()
    {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        bytes[..8].copy_from_slice(&epoch.to_le_bytes());
        bytes[8..].copy_from_slice(&(nanos as u64).to_le_bytes());
    }
    let mut token = String::from("tok_");
    for byte in bytes {
        token.push_str(&format!("{byte:02x}"));
    }
    token.push('_');
    token.push_str(&epoch.to_string());
    baidupcsr_preview::VideoToken::new(token)
}

fn preview_key_for_entry(
    account_uid: u64,
    entry: &baidupcsr_pcs::FileEntry,
) -> baidupcsr_preview::PreviewKey {
    baidupcsr_preview::PreviewKey {
        account_uid,
        fs_id: entry.fs_id,
        mtime: entry.mtime,
        size: nonnegative_size(entry.size),
    }
}

fn nonnegative_size(size: i64) -> u64 {
    u64::try_from(size).unwrap_or(0)
}

fn preview_budget_from_status(
    status: baidupcsr_preview::PreviewStatus,
) -> baidupcsr_tui::PreviewBudget {
    baidupcsr_tui::PreviewBudget {
        kind: tui_preview_kind(status.kind),
        fetched_bytes: status.fetched_bytes,
        memory_limit_bytes: status.memory_limit_bytes,
        truncated: status.truncated,
        disk_backed: status.disk_backed,
    }
}

fn tui_preview_kind(kind: baidupcsr_preview::PreviewKind) -> baidupcsr_tui::PreviewKind {
    match kind {
        baidupcsr_preview::PreviewKind::Text => baidupcsr_tui::PreviewKind::Text,
        baidupcsr_preview::PreviewKind::Image => baidupcsr_tui::PreviewKind::Image,
        baidupcsr_preview::PreviewKind::Metadata => baidupcsr_tui::PreviewKind::Metadata,
        baidupcsr_preview::PreviewKind::Video => baidupcsr_tui::PreviewKind::Video,
    }
}

fn pcs_user_agent_for_config(config: &baidupcsr_config::PcsConfig) -> &str {
    if !config.pcs_ua.is_empty() {
        config.pcs_ua.as_str()
    } else if !config.user_agent.is_empty() {
        config.user_agent.as_str()
    } else if !config.pan_ua.is_empty() {
        config.pan_ua.as_str()
    } else {
        "netdisk"
    }
}

fn pan_user_agent_for_config(config: &baidupcsr_config::PcsConfig) -> &str {
    if !config.pan_ua.is_empty() {
        config.pan_ua.as_str()
    } else if !config.user_agent.is_empty() {
        config.user_agent.as_str()
    } else if !config.pcs_ua.is_empty() {
        config.pcs_ua.as_str()
    } else {
        "netdisk"
    }
}

fn bduss_from_user(user: &baidupcsr_config::BaiduUser) -> Option<String> {
    if !user.bduss.is_empty() {
        return Some(user.bduss.clone());
    }

    cookie_value(user.cookies.as_str(), "BDUSS").map(str::to_string)
}

fn cookie_value<'a>(cookies: &'a str, key: &str) -> Option<&'a str> {
    cookies.split(';').find_map(|item| {
        let (name, value) = item.trim().split_once('=')?;
        if name == key && !value.is_empty() {
            Some(value)
        } else {
            None
        }
    })
}

fn optional_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn print_doctor(config_dir: Option<PathBuf>) {
    println!("baidupcsr doctor");
    println!("redacted: true");
    let discovery = baidupcsr_config::discovery_input_from_env(config_dir);
    let config_path = baidupcsr_config::discover_config_file(&discovery);
    println!("config_path: {}", config_path.display());

    match baidupcsr_config::load_config(&config_path) {
        Ok(config) => print_loaded_config(&config),
        Err(err) => {
            println!("config: not loaded");
            println!("config_error: {err}");
        }
    }

    println!("crates:");
    println!("  - {}", baidupcsr_config::crate_name());
    println!("  - {}", baidupcsr_core::crate_name());
    println!("  - {}", baidupcsr_pcs::crate_name());
    println!("  - {}", baidupcsr_preview::crate_name());
    println!("  - {}", baidupcsr_tui::crate_name());
}

fn print_loaded_config(config: &baidupcsr_config::PcsConfig) {
    println!("config: loaded");
    println!("active_uid: {}", config.baidu_active_uid);
    println!("accounts: {}", config.baidu_user_list.len());

    match baidupcsr_config::active_user(config) {
        Some(user) => {
            let auth = user.auth_presence();
            println!("workdir: {}", user.workdir);
            println!("bduss: {}", presence(auth.bduss));
            println!("stoken: {}", presence(auth.stoken));
            println!("cookies: {}", presence(auth.cookies));
            println!("access_token: {}", presence(auth.access_token));
        }
        None => {
            println!("active_user: missing");
        }
    }
}

fn presence(value: bool) -> &'static str {
    if value {
        "present"
    } else {
        "missing"
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use baidupcsr_pcs::FileEntry;

    use super::{
        bduss_from_user, build_pcs_client, is_video_entry, list_remote_directory,
        pan_user_agent_for_config, preview_key_for_entry, tui_preview_kind,
        video_budget_from_config,
    };

    fn config_fixture(name: &str) -> Result<baidupcsr_config::PcsConfig, String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config")
            .join(name);
        baidupcsr_config::load_config(&path).map_err(|err| err.to_string())
    }

    fn file(fs_id: i64, size: i64) -> FileEntry {
        FileEntry {
            fs_id,
            app_id: 266719,
            path: "/docs/notes.md".to_string(),
            server_filename: "notes.md".to_string(),
            ctime: 1,
            mtime: 2,
            md5: String::new(),
            block_list: Vec::new(),
            size,
            is_dir: false,
            has_subdir: false,
        }
    }

    fn pcs_error_kind(err: &baidupcsr_pcs::PcsError) -> String {
        match err {
            baidupcsr_pcs::PcsError::Cancelled { .. } => "cancelled".to_string(),
            baidupcsr_pcs::PcsError::Parse { message, .. } => format!("parse:{message}"),
            baidupcsr_pcs::PcsError::Network { .. } => "network".to_string(),
            baidupcsr_pcs::PcsError::Remote { code, .. } => format!("remote_code={code}"),
        }
    }

    #[test]
    #[ignore = "uses the machine-local BaiduPCS-Go config and network; prints no secret fields"]
    fn live_video_dlink_range_smoke_is_redacted() -> Result<(), String> {
        let discovery = baidupcsr_config::discovery_input_from_env(None);
        let config_path = baidupcsr_config::discover_config_file(&discovery);
        let config = baidupcsr_config::load_config(&config_path)
            .map_err(|err| format!("live_config_load_failed: {err}"))?;
        let user = baidupcsr_config::active_user(&config)
            .ok_or_else(|| "live_active_user_missing".to_string())?;
        let bduss = bduss_from_user(user).ok_or_else(|| "live_bduss_missing".to_string())?;
        let client = build_pcs_client(&config, user)
            .map_err(|err| format!("live_client_build_failed: {err}"))?;
        let entries = list_remote_directory(&client, user.workdir.as_str())
            .map_err(|err| format!("live_list_failed: {err}"))?;
        let entry = entries
            .into_iter()
            .find(is_video_entry)
            .ok_or_else(|| "live_video_missing".to_string())?;

        let urls = client
            .locate_download_urls(entry.path.as_str(), user.uid, bduss.as_str())
            .map_err(|err| format!("live_locatedownload_failed: {}", pcs_error_kind(&err)))?;
        println!("live_locatedownload_url_count={}", urls.len());
        let first_url = urls
            .first()
            .ok_or_else(|| "live_locatedownload_empty".to_string())?;

        let range = baidupcsr_pcs::ByteRange::new(0, 1023)
            .map_err(|err| format!("live_range_invalid: {}", pcs_error_kind(&err)))?;
        let bytes = client
            .download_url_range_with_cancel(first_url.as_str(), range, || false)
            .map_err(|err| format!("live_dlink_range_failed: {}", pcs_error_kind(&err)))?;

        println!("live_dlink_range_bytes={}", bytes.len());
        assert!(!bytes.is_empty());
        assert!(bytes.len() <= 1025);
        Ok(())
    }

    #[test]
    fn bduss_from_user_prefers_explicit_field() -> Result<(), String> {
        let config = config_fixture("pcs_config_minimal.json")?;
        let user = baidupcsr_config::active_user(&config)
            .ok_or_else(|| "fixture active user missing".to_string())?;

        assert_eq!(bduss_from_user(user), Some("FAKE_BDUSS".to_string()));
        Ok(())
    }

    #[test]
    fn bduss_from_user_extracts_cookie_value() -> Result<(), String> {
        let config = config_fixture("pcs_config_cookie_only.json")?;
        let user = baidupcsr_config::active_user(&config)
            .ok_or_else(|| "fixture active user missing".to_string())?;

        assert_eq!(bduss_from_user(user), Some("FAKE_COOKIE_BDUSS".to_string()));
        Ok(())
    }

    #[test]
    fn pan_user_agent_prefers_pan_ua_for_locatedownload() -> Result<(), String> {
        let config = config_fixture("pcs_config_minimal.json")?;

        assert_eq!(pan_user_agent_for_config(&config), "fixture-pan-ua");
        Ok(())
    }

    #[test]
    fn preview_key_uses_account_file_mtime_and_nonnegative_size() {
        let key = preview_key_for_entry(42, &file(7, -1));

        assert_eq!(key.account_uid, 42);
        assert_eq!(key.fs_id, 7);
        assert_eq!(key.mtime, 2);
        assert_eq!(key.size, 0);
    }

    #[test]
    fn preview_kind_mapping_preserves_text_and_metadata_boundaries() {
        assert_eq!(
            tui_preview_kind(baidupcsr_preview::PreviewKind::Text),
            baidupcsr_tui::PreviewKind::Text
        );
        assert_eq!(
            tui_preview_kind(baidupcsr_preview::PreviewKind::Metadata),
            baidupcsr_tui::PreviewKind::Metadata
        );
    }

    #[test]
    fn video_entry_detection_requires_video_file_extension() {
        let mut entry = file(8, 1024);
        entry.server_filename = "clip.mp4".to_string();
        assert!(is_video_entry(&entry));

        entry.server_filename = "notes.txt".to_string();
        assert!(!is_video_entry(&entry));

        entry.server_filename = "folder.mp4".to_string();
        entry.is_dir = true;
        assert!(!is_video_entry(&entry));
    }

    #[test]
    fn video_budget_uses_sidecar_values() -> Result<(), baidupcsr_preview::VideoPreviewError> {
        let config = baidupcsr_config::VideoPreviewConfig {
            enabled: true,
            chunk_size_bytes: 128 * 1024,
            max_chunks: 3,
        };
        let budget = video_budget_from_config(&config)?;

        assert_eq!(budget.chunk_size_bytes, 128 * 1024);
        assert_eq!(budget.max_chunks, 3);
        assert_eq!(budget.memory_limit_bytes(), 384 * 1024);
        Ok(())
    }
}
