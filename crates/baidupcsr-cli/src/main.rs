use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PREVIEW_MEMORY_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
const TEXT_PREVIEW_FETCH_BYTES: u64 = 64 * 1024;
const DOWNLOAD_CHUNK_BYTES: u64 = 1024 * 1024;
const ENV_DOWNLOAD_DIR: &str = "BAIDUPCSR_DOWNLOAD_DIR";
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
    cache_store: baidupcsr_core::DirectoryCacheStore,
    loaded_at: HashMap<String, SystemTime>,
    cache_sources: HashMap<String, baidupcsr_tui::CacheSource>,
    selection_memory: HashMap<String, String>,
    preload_planner: baidupcsr_core::PagedPreloadPlanner,
    preview_manager: baidupcsr_preview::PreviewManager,
    preview_epoch: Arc<AtomicU64>,
    video_config: baidupcsr_config::VideoPreviewConfig,
    video_epoch: Arc<AtomicU64>,
    active_video: Option<ActiveVideoPreview>,
    download_dir: PathBuf,
    task_manager: baidupcsr_core::TaskManager,
    task_cancellations: HashMap<baidupcsr_core::TaskId, Arc<AtomicBool>>,
    pending_remote_operation: Option<baidupcsr_core::RemoteOperationPlan>,
    tx: mpsc::Sender<DirectoryLoadResult>,
    rx: mpsc::Receiver<DirectoryLoadResult>,
    preview_tx: mpsc::Sender<PreviewLoadResult>,
    preview_rx: mpsc::Receiver<PreviewLoadResult>,
    task_tx: mpsc::Sender<TransferTaskEvent>,
    task_rx: mpsc::Receiver<TransferTaskEvent>,
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
    kind: PreviewFetchKind,
    title: String,
    result: Result<Vec<u8>, String>,
}

struct PreviewLoadJob {
    client: baidupcsr_pcs::BlockingPcsClient,
    path: String,
    title: String,
    request: baidupcsr_preview::PreviewRequest,
    kind: PreviewFetchKind,
    fetch_bytes: u64,
    preview_epoch: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreviewFetchKind {
    Text,
    ImageMetadata,
}

struct ActiveVideoPreview {
    key: baidupcsr_preview::PreviewKey,
    title: String,
    server: baidupcsr_preview::VideoLoopbackServer,
    player: Child,
}

enum TransferTaskEvent {
    Progress {
        id: baidupcsr_core::TaskId,
        bytes_done: u64,
        elapsed_seconds: u64,
    },
    Completed {
        id: baidupcsr_core::TaskId,
    },
    Failed {
        id: baidupcsr_core::TaskId,
        message: String,
    },
    Cancelled {
        id: baidupcsr_core::TaskId,
        message: String,
    },
    RemoteCompleted {
        id: baidupcsr_core::TaskId,
        label: String,
        affected_dirs: Vec<String>,
    },
    RemoteFailed {
        id: baidupcsr_core::TaskId,
        message: String,
    },
    SearchCompleted {
        id: baidupcsr_core::TaskId,
        path: String,
        keyword: String,
        result: Result<Vec<baidupcsr_pcs::FileEntry>, String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ParsedRemoteCommand {
    Operation(baidupcsr_core::RemoteOperationPlan),
    Search {
        path: String,
        keyword: String,
        recursive: bool,
    },
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
        let (task_tx, task_rx) = mpsc::channel();
        let ui_config_path = baidupcsr_config::discover_ui_config_file();
        let ui_config = baidupcsr_config::load_ui_config_or_default(&ui_config_path)
            .unwrap_or_else(|_| baidupcsr_config::UiConfig::default());
        let cache_store =
            baidupcsr_core::DirectoryCacheStore::new(baidupcsr_config::discover_cache_dir());
        let preload_planner = baidupcsr_core::PagedPreloadPlanner::new(18, 1, 4, 4);
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
                    cache_store: cache_store.clone(),
                    loaded_at: HashMap::new(),
                    cache_sources: HashMap::new(),
                    selection_memory: HashMap::new(),
                    preload_planner,
                    preview_manager: baidupcsr_preview::PreviewManager::new(
                        PREVIEW_MEMORY_LIMIT_BYTES,
                    ),
                    preview_epoch: Arc::new(AtomicU64::new(0)),
                    video_config: ui_config.video_preview,
                    video_epoch: Arc::new(AtomicU64::new(0)),
                    active_video: None,
                    download_dir: PathBuf::from("."),
                    task_manager: baidupcsr_core::TaskManager::new(),
                    task_cancellations: HashMap::new(),
                    pending_remote_operation: None,
                    tx,
                    rx,
                    preview_tx,
                    preview_rx,
                    task_tx,
                    task_rx,
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
                cache_store: cache_store.clone(),
                loaded_at: HashMap::new(),
                cache_sources: HashMap::new(),
                selection_memory: HashMap::new(),
                preload_planner,
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
                video_config: ui_config.video_preview,
                video_epoch: Arc::new(AtomicU64::new(0)),
                active_video: None,
                download_dir: download_dir_for_config(&config),
                task_manager: baidupcsr_core::TaskManager::new(),
                task_cancellations: HashMap::new(),
                pending_remote_operation: None,
                tx,
                rx,
                preview_tx,
                preview_rx,
                task_tx,
                task_rx,
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
        let download_dir = download_dir_for_config(&config);

        match client {
            Ok(client) => Self {
                client: Some(client),
                account_uid: user.uid,
                account_bduss,
                cache: baidupcsr_core::DirectoryCache::new(),
                cache_store: cache_store.clone(),
                loaded_at: HashMap::new(),
                cache_sources: HashMap::new(),
                selection_memory: HashMap::new(),
                preload_planner,
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
                video_config: ui_config.video_preview,
                video_epoch: Arc::new(AtomicU64::new(0)),
                active_video: None,
                download_dir,
                task_manager: baidupcsr_core::TaskManager::new(),
                task_cancellations: HashMap::new(),
                pending_remote_operation: None,
                tx,
                rx,
                preview_tx,
                preview_rx,
                task_tx,
                task_rx,
                inflight: 0,
                initial_path,
                startup_notice: None,
            },
            Err(message) => Self {
                client: None,
                account_uid: user.uid,
                account_bduss,
                cache: baidupcsr_core::DirectoryCache::new(),
                cache_store,
                loaded_at: HashMap::new(),
                cache_sources: HashMap::new(),
                selection_memory: HashMap::new(),
                preload_planner,
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
                video_config: ui_config.video_preview,
                video_epoch: Arc::new(AtomicU64::new(0)),
                active_video: None,
                download_dir,
                task_manager: baidupcsr_core::TaskManager::new(),
                task_cancellations: HashMap::new(),
                pending_remote_operation: None,
                tx,
                rx,
                preview_tx,
                preview_rx,
                task_tx,
                task_rx,
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
                self.drain_transfer_task_events(app);
                self.refresh_active_video_preview(app);
                self.refresh_current_cache_age(app);
                self.refresh_task_panel(app);
            }
            baidupcsr_tui::TuiRuntimeEvent::Action(action) => self.handle_action(app, action),
            baidupcsr_tui::TuiRuntimeEvent::CommandSubmitted(command) => {
                self.handle_remote_command(app, command);
            }
            baidupcsr_tui::TuiRuntimeEvent::ConfirmAccepted => {
                self.confirm_pending_remote_operation(app);
            }
            baidupcsr_tui::TuiRuntimeEvent::ConfirmRejected => {
                self.pending_remote_operation = None;
                app.set_notice("operation_cancelled");
            }
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
                self.update_preview_for_selection(app);
                self.prefetch_visible_page(app);
            }
            baidupcsr_tui::KeyAction::Preview => {
                self.start_video_preview(app);
            }
            baidupcsr_tui::KeyAction::Download => {
                self.start_download_task(app);
            }
            baidupcsr_tui::KeyAction::CancelTask => {
                self.cancel_latest_task(app);
            }
            baidupcsr_tui::KeyAction::Search
            | baidupcsr_tui::KeyAction::Command
            | baidupcsr_tui::KeyAction::ToggleSelect
            | baidupcsr_tui::KeyAction::Quit
            | baidupcsr_tui::KeyAction::None => {}
        }
        self.refresh_task_panel(app);
    }

    fn handle_remote_command(&mut self, app: &mut baidupcsr_tui::TuiApp, command: String) {
        match parse_remote_command(command.as_str(), app) {
            Ok(ParsedRemoteCommand::Search {
                path,
                keyword,
                recursive,
            }) => self.start_search_task(app, path, keyword, recursive),
            Ok(ParsedRemoteCommand::Operation(plan)) if plan.is_destructive() => {
                self.pending_remote_operation = Some(plan.clone());
                let mut lines = plan.dry_run_lines().to_vec();
                lines.push("press y to confirm, n to cancel".to_string());
                app.begin_confirm(lines);
            }
            Ok(ParsedRemoteCommand::Operation(plan)) => {
                self.start_remote_operation_task(app, plan);
            }
            Err(message) => app.set_notice(format!("command_error: {message}")),
        }
        self.refresh_task_panel(app);
    }

    fn confirm_pending_remote_operation(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        let Some(plan) = self.pending_remote_operation.take() else {
            app.set_notice("operation_confirm: no pending operation");
            return;
        };
        self.start_remote_operation_task(app, plan);
    }

    fn start_remote_operation_task(
        &mut self,
        app: &mut baidupcsr_tui::TuiApp,
        plan: baidupcsr_core::RemoteOperationPlan,
    ) {
        let Some(client) = self.client.clone() else {
            app.set_notice("operation_error: client unavailable");
            return;
        };
        let label = plan.label().to_string();
        let affected_dirs = plan.affected_dirs().to_vec();
        let id = self
            .task_manager
            .enqueue_remote_operation(label.as_str(), plan.item_count());
        if self.task_manager.start(id).is_err() {
            app.set_notice("operation_error: failed to start task");
            return;
        }

        let tx = self.task_tx.clone();
        let notice_label = label.clone();
        thread::spawn(move || {
            let event = match execute_remote_operation(&client, &plan) {
                Ok(()) => TransferTaskEvent::RemoteCompleted {
                    id,
                    label,
                    affected_dirs,
                },
                Err(message) => TransferTaskEvent::RemoteFailed { id, message },
            };
            let _ = tx.send(event);
        });
        app.set_notice(format!(
            "operation_started: #{} {}",
            id.value(),
            notice_label
        ));
    }

    fn start_search_task(
        &mut self,
        app: &mut baidupcsr_tui::TuiApp,
        path: String,
        keyword: String,
        recursive: bool,
    ) {
        let Some(client) = self.client.clone() else {
            app.set_notice("search_error: client unavailable");
            return;
        };
        let id = self.task_manager.enqueue_remote_operation("search", 1);
        if self.task_manager.start(id).is_err() {
            app.set_notice("search_error: failed to start task");
            return;
        }

        let tx = self.task_tx.clone();
        let task_path = path.clone();
        let task_keyword = keyword.clone();
        thread::spawn(move || {
            let result = client
                .search(task_path.as_str(), task_keyword.as_str(), recursive)
                .map_err(|err| pcs_error_label(&err));
            let _ = tx.send(TransferTaskEvent::SearchCompleted {
                id,
                path: task_path,
                keyword: task_keyword,
                result,
            });
        });
        app.set_notice(format!("search_started: #{} {}", id.value(), keyword));
    }

    fn start_directory_load(
        &mut self,
        app: &mut baidupcsr_tui::TuiApp,
        path: String,
        preferred_selection_path: Option<String>,
    ) {
        self.stop_active_video();
        self.hydrate_directory_cache_from_disk(path.as_str());
        let view = self.cache.enter_directory(path.as_str());
        let cache_age = self.cache_age_seconds(path.as_str());
        app.set_cache_source(self.cache_source_for_path(path.as_str()));
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
        self.prefetch_visible_page(app);
    }

    fn prefetch_visible_page(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        let selected = app.selected_index().unwrap_or(0);
        let plan = self.preload_planner.plan(app.entries(), selected);
        let directories = plan
            .targets
            .iter()
            .filter(|target| target.kind == baidupcsr_core::PreloadKind::Directory)
            .count();
        let previews = plan
            .targets
            .iter()
            .filter(|target| target.kind == baidupcsr_core::PreloadKind::Preview)
            .count();
        app.set_preload_lines(vec![
            format!("queued dirs={directories} previews={previews}"),
            format!("window={}..{}", plan.window_start, plan.window_end),
        ]);

        for target in plan.targets {
            match target.kind {
                baidupcsr_core::PreloadKind::Directory => {
                    self.prefetch_directory_path(app, target.path);
                }
                baidupcsr_core::PreloadKind::Preview => {
                    self.prefetch_preview_path(app, target.path.as_str());
                }
            }
        }
    }

    fn prefetch_directory_path(&mut self, app: &mut baidupcsr_tui::TuiApp, path: String) {
        self.hydrate_directory_cache_from_disk(path.as_str());
        let source = self.cache_source_for_path(path.as_str());
        let status = self.cache.snapshot(path.as_str()).status;
        if status == baidupcsr_core::DirectoryStatus::Refreshing {
            return;
        }
        if status == baidupcsr_core::DirectoryStatus::Fresh
            && source == baidupcsr_tui::CacheSource::Network
        {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let generation = self.cache.begin_refresh(path.as_str());
        self.spawn_directory_load(client, path, generation);
        app.set_background_tasks(self.inflight);
    }

    fn prefetch_preview_path(&mut self, app: &mut baidupcsr_tui::TuiApp, path: &str) {
        let Some(entry) = app
            .entries()
            .iter()
            .find(|entry| entry.path == path)
            .cloned()
        else {
            return;
        };
        if entry.is_dir {
            return;
        }
        let size = nonnegative_size(entry.size);
        let plan = baidupcsr_preview::PreviewPlan::for_entry(
            entry.server_filename.as_str(),
            entry.is_dir,
            size,
            PREVIEW_MEMORY_LIMIT_BYTES,
        );
        if plan.action != baidupcsr_preview::PreviewAction::FetchText {
            return;
        }
        let key = preview_key_for_entry(self.account_uid, &entry);
        if self.preview_manager.get(&key).is_some() {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let request = self.preview_manager.start_request(key);
        let fetch_bytes = size.min(TEXT_PREVIEW_FETCH_BYTES);
        if fetch_bytes == 0 {
            return;
        }
        let preview_epoch = self.preview_epoch.load(Ordering::Acquire);
        self.spawn_preview_load(PreviewLoadJob {
            client,
            path: entry.path,
            title: entry.server_filename,
            request,
            kind: PreviewFetchKind::Text,
            fetch_bytes,
            preview_epoch,
        });
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
                    let entries_for_store = entries.clone();
                    let refresh =
                        self.cache
                            .finish_refresh(path.as_str(), result.generation, entries);
                    if refresh.applied {
                        self.loaded_at.insert(path.clone(), SystemTime::now());
                        self.cache_sources
                            .insert(path.clone(), baidupcsr_tui::CacheSource::Network);
                        self.persist_directory_cache(path.as_str(), entries_for_store);
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
                            app.set_cache_source(self.cache_source_for_path(path.as_str()));
                            self.update_preview_for_selection(app);
                            self.prefetch_visible_page(app);
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
                        app.set_cache_source(self.cache_source_for_path(path.as_str()));
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

        match plan.action {
            baidupcsr_preview::PreviewAction::MetadataOnly => {
                app.finish_preview_metadata(
                    entry.server_filename.as_str(),
                    tui_preview_kind(plan.kind),
                    plan.reason,
                    budget,
                );
            }
            baidupcsr_preview::PreviewAction::FetchText => {
                self.start_text_preview_load(app, entry, size, preview_epoch);
            }
            baidupcsr_preview::PreviewAction::FetchImageMetadata => {
                self.start_image_metadata_preview_load(app, entry, size, preview_epoch);
            }
        }
    }

    fn start_text_preview_load(
        &mut self,
        app: &mut baidupcsr_tui::TuiApp,
        entry: baidupcsr_pcs::FileEntry,
        size: u64,
        preview_epoch: u64,
    ) {
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

        self.spawn_preview_load(PreviewLoadJob {
            client,
            path: entry.path,
            title: entry.server_filename,
            request,
            kind: PreviewFetchKind::Text,
            fetch_bytes,
            preview_epoch,
        });
        app.set_background_tasks(self.inflight);
    }

    fn start_image_metadata_preview_load(
        &mut self,
        app: &mut baidupcsr_tui::TuiApp,
        entry: baidupcsr_pcs::FileEntry,
        size: u64,
        preview_epoch: u64,
    ) {
        let Some(client) = self.client.clone() else {
            app.finish_preview_error(entry.server_filename.as_str(), "client unavailable");
            return;
        };
        let key = preview_key_for_entry(self.account_uid, &entry);
        let request = self.preview_manager.start_request(key);
        app.begin_preview(entry.server_filename.as_str(), "loading image metadata");
        let fetch_bytes = size.min(TEXT_PREVIEW_FETCH_BYTES);
        if fetch_bytes == 0 {
            app.finish_preview_image_metadata(
                entry.server_filename.as_str(),
                vec!["image=empty".to_string()],
                baidupcsr_tui::PreviewBudget {
                    kind: baidupcsr_tui::PreviewKind::Image,
                    fetched_bytes: 0,
                    memory_limit_bytes: PREVIEW_MEMORY_LIMIT_BYTES,
                    truncated: false,
                    disk_backed: false,
                },
            );
            return;
        }

        self.spawn_preview_load(PreviewLoadJob {
            client,
            path: entry.path,
            title: entry.server_filename,
            request,
            kind: PreviewFetchKind::ImageMetadata,
            fetch_bytes,
            preview_epoch,
        });
        app.set_background_tasks(self.inflight);
    }

    fn spawn_preview_load(&mut self, job: PreviewLoadJob) {
        self.inflight = self.inflight.saturating_add(1);
        let tx = self.preview_tx.clone();
        let cancellation_epoch = Arc::clone(&self.preview_epoch);
        thread::spawn(move || {
            let range = baidupcsr_pcs::ByteRange::prefix(job.fetch_bytes);
            let result = job
                .client
                .download_range_with_cancel(job.path.as_str(), range, || {
                    cancellation_epoch.load(Ordering::Acquire) != job.preview_epoch
                })
                .map_err(|err| format!("preview_error: {err}"));
            let _ = tx.send(PreviewLoadResult {
                request: job.request,
                kind: job.kind,
                title: job.title,
                result,
            });
        });
    }

    fn drain_preview_results(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        while let Ok(result) = self.preview_rx.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            match result.result {
                Ok(bytes) if result.kind == PreviewFetchKind::Text => {
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
                Ok(bytes) => {
                    let key = result.request.key();
                    if self.preview_manager.request_state(result.request)
                        == baidupcsr_preview::RequestState::Applied
                        && self.selected_preview_key(app) == Some(key)
                    {
                        let metadata = baidupcsr_preview::parse_image_metadata(&bytes);
                        app.finish_preview_image_metadata(
                            result.title.as_str(),
                            image_preview_lines(metadata),
                            baidupcsr_tui::PreviewBudget {
                                kind: baidupcsr_tui::PreviewKind::Image,
                                fetched_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                                memory_limit_bytes: PREVIEW_MEMORY_LIMIT_BYTES,
                                truncated: false,
                                disk_backed: false,
                            },
                        );
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

    fn start_download_task(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        let Some(entry) = app.selected_entry().cloned() else {
            app.set_notice("download: no selected entry");
            return;
        };
        if entry.is_dir {
            app.set_notice("download: directories are not downloadable in this stage");
            return;
        }
        let Some(client) = self.client.clone() else {
            app.set_notice("download_error: client unavailable");
            return;
        };
        let Some(account_bduss) = self.account_bduss.clone() else {
            app.set_notice("download_auth_error: missing BDUSS");
            return;
        };
        if self.account_uid == 0 {
            app.set_notice("download_auth_error: missing uid");
            return;
        }

        let size = nonnegative_size(entry.size);
        let target = download_target_path(&self.download_dir, entry.server_filename.as_str());
        let target_label = file_name_label(target.to_string_lossy().as_ref());
        let id = self.task_manager.enqueue_download(
            entry.path.as_str(),
            target.to_string_lossy().as_ref(),
            size,
        );
        if self.task_manager.start(id).is_err() {
            app.set_notice("download_error: failed to start task");
            return;
        }

        let cancellation = Arc::new(AtomicBool::new(false));
        self.task_cancellations
            .insert(id, Arc::clone(&cancellation));
        let tx = self.task_tx.clone();
        let remote_path = entry.path;
        let account_uid = self.account_uid;
        thread::spawn(move || {
            run_download_worker(DownloadWorker {
                id,
                client,
                remote_path,
                target,
                size,
                account_uid,
                account_bduss,
                cancellation,
                tx,
            });
        });
        app.set_notice(format!(
            "download_started: #{} {}",
            id.value(),
            target_label
        ));
    }

    fn cancel_latest_task(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        let Some(id) = self
            .task_manager
            .snapshots()
            .into_iter()
            .rev()
            .find(|task| {
                matches!(
                    task.state,
                    baidupcsr_core::TaskState::Queued | baidupcsr_core::TaskState::Running
                )
            })
            .map(|task| task.id)
        else {
            app.set_notice("task_cancel: no active transfer task");
            return;
        };

        if self.task_manager.request_cancel(id).is_err() {
            app.set_notice(format!("task_cancel_error: #{}", id.value()));
            return;
        }
        if let Some(cancellation) = self.task_cancellations.get(&id) {
            cancellation.store(true, Ordering::Release);
        }
        app.set_notice(format!("task_cancel_requested: #{}", id.value()));
    }

    fn drain_transfer_task_events(&mut self, app: &mut baidupcsr_tui::TuiApp) {
        while let Ok(event) = self.task_rx.try_recv() {
            match event {
                TransferTaskEvent::Progress {
                    id,
                    bytes_done,
                    elapsed_seconds,
                } => {
                    let _ = self
                        .task_manager
                        .update_progress(id, bytes_done, elapsed_seconds);
                }
                TransferTaskEvent::Completed { id } => {
                    let _ = self.task_manager.complete(id);
                    self.task_cancellations.remove(&id);
                    app.set_notice(format!("download_complete: #{}", id.value()));
                    let preferred = selected_path_for_refresh(app);
                    self.start_directory_load(app, app.path().to_string(), preferred);
                }
                TransferTaskEvent::Failed { id, message } => {
                    let _ = self.task_manager.fail(id, message.as_str());
                    self.task_cancellations.remove(&id);
                    app.set_notice(format!("download_failed: #{} {}", id.value(), message));
                }
                TransferTaskEvent::Cancelled { id, message } => {
                    let _ = self.task_manager.cancelled(id, message.as_str());
                    self.task_cancellations.remove(&id);
                    app.set_notice(format!("download_cancelled: #{}", id.value()));
                }
                TransferTaskEvent::RemoteCompleted {
                    id,
                    label,
                    affected_dirs,
                } => {
                    let _ = self.task_manager.complete(id);
                    app.set_notice(format!("operation_complete: #{} {}", id.value(), label));
                    self.refresh_related_dirs(app, affected_dirs);
                }
                TransferTaskEvent::RemoteFailed { id, message } => {
                    let _ = self.task_manager.fail(id, message.as_str());
                    app.set_notice(format!("operation_failed: #{} {}", id.value(), message));
                }
                TransferTaskEvent::SearchCompleted {
                    id,
                    path,
                    keyword,
                    result,
                } => match result {
                    Ok(entries) => {
                        let count = entries.len();
                        let _ = self.task_manager.complete(id);
                        app.set_entries(entries);
                        app.set_notice(format!(
                            "search_complete: #{} {} matches for {} in {}",
                            id.value(),
                            count,
                            keyword,
                            path
                        ));
                        self.update_preview_for_selection(app);
                    }
                    Err(message) => {
                        let _ = self.task_manager.fail(id, message.as_str());
                        app.set_notice(format!("search_failed: #{} {}", id.value(), message));
                    }
                },
            }
        }
    }

    fn refresh_related_dirs(&mut self, app: &mut baidupcsr_tui::TuiApp, dirs: Vec<String>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let mut refreshed_current = false;
        for dir in dirs {
            if dir == app.path() {
                if !refreshed_current {
                    refreshed_current = true;
                    let preferred = selected_path_for_refresh(app);
                    self.start_directory_load(app, dir, preferred);
                }
            } else {
                let generation = self.cache.begin_refresh(dir.as_str());
                self.spawn_directory_load(client.clone(), dir, generation);
            }
        }
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
        let launch = match server.player_launch_with_profile(
            baidupcsr_preview::PlayerBackend::Mpv,
            mpv_profile_from_config(self.video_config.player_profile),
        ) {
            Ok(launch) => launch,
            Err(err) => {
                app.finish_preview_error(
                    entry.server_filename.as_str(),
                    format!("video_player_error: {err:?}"),
                );
                return;
            }
        };
        if !program_available(launch.program()) {
            app.finish_preview_error(
                entry.server_filename.as_str(),
                format!(
                    "video_player_missing: {} not found on PATH",
                    launch.program()
                ),
            );
            return;
        }
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
                "mpv_profile={}",
                mpv_profile_label(self.video_config.player_profile)
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
        app.set_cache_source(self.cache_source_for_path(app.path()));
    }

    fn refresh_task_panel(&self, app: &mut baidupcsr_tui::TuiApp) {
        let lines = self
            .task_manager
            .snapshots()
            .into_iter()
            .rev()
            .take(3)
            .map(|task| task_summary_line(&task))
            .collect();
        app.set_task_lines(lines);
        app.set_background_tasks(self.inflight + self.task_manager.active_count());
    }

    fn cache_age_seconds(&self, path: &str) -> Option<u64> {
        self.loaded_at
            .get(path)
            .and_then(|loaded_at| SystemTime::now().duration_since(*loaded_at).ok())
            .map(|duration| duration.as_secs())
    }

    fn cache_source_for_path(&self, path: &str) -> baidupcsr_tui::CacheSource {
        self.cache_sources
            .get(path)
            .copied()
            .unwrap_or(baidupcsr_tui::CacheSource::None)
    }

    fn hydrate_directory_cache_from_disk(&mut self, path: &str) {
        if self.cache.snapshot(path).status != baidupcsr_core::DirectoryStatus::Empty {
            return;
        }
        let snapshot = match self.cache_store.read_snapshot(self.account_uid, path) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) | Err(_) => return,
        };
        self.loaded_at.insert(
            path.to_string(),
            UNIX_EPOCH + Duration::from_secs(snapshot.saved_at_unix_seconds),
        );
        self.cache_sources
            .insert(path.to_string(), baidupcsr_tui::CacheSource::Disk);
        self.cache.seed(path, snapshot.entries);
    }

    fn persist_directory_cache(&self, path: &str, entries: Vec<baidupcsr_pcs::FileEntry>) {
        let store = self.cache_store.clone();
        let path = path.to_string();
        let account_uid = self.account_uid;
        let saved_at = unix_timestamp_now();
        thread::spawn(move || {
            let _ = store.write_snapshot(account_uid, path.as_str(), entries, saved_at);
        });
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

struct DownloadWorker {
    id: baidupcsr_core::TaskId,
    client: baidupcsr_pcs::BlockingPcsClient,
    remote_path: String,
    target: PathBuf,
    size: u64,
    account_uid: u64,
    account_bduss: String,
    cancellation: Arc<AtomicBool>,
    tx: mpsc::Sender<TransferTaskEvent>,
}

enum DownloadWorkerResult {
    Completed,
    Cancelled(String),
    Failed(String),
}

#[derive(Debug, Eq, PartialEq)]
enum DlinkRangeAttempt {
    Bytes(Vec<u8>),
    Cancelled,
    Failed(String),
}

fn run_download_worker(worker: DownloadWorker) {
    let id = worker.id;
    let tx = worker.tx.clone();
    let result = download_worker_body(worker);
    let event = match result {
        DownloadWorkerResult::Completed => TransferTaskEvent::Completed { id },
        DownloadWorkerResult::Cancelled(message) => TransferTaskEvent::Cancelled { id, message },
        DownloadWorkerResult::Failed(message) => TransferTaskEvent::Failed { id, message },
    };
    let _ = tx.send(event);
}

fn download_worker_body(worker: DownloadWorker) -> DownloadWorkerResult {
    if worker.cancellation.load(Ordering::Acquire) {
        return DownloadWorkerResult::Cancelled("user cancelled".to_string());
    }
    if worker.target.exists() {
        return DownloadWorkerResult::Failed("local_exists".to_string());
    }
    if let Some(parent) = worker.target.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            return DownloadWorkerResult::Failed(io_error_label(&err));
        }
    }
    if worker.size == 0 {
        return match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&worker.target)
        {
            Ok(_) => DownloadWorkerResult::Completed,
            Err(err) => DownloadWorkerResult::Failed(io_error_label(&err)),
        };
    }

    let partial = partial_download_path(&worker.target);
    let mut offset = match fs::metadata(&partial) {
        Ok(metadata) => metadata.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
        Err(err) => return DownloadWorkerResult::Failed(io_error_label(&err)),
    };
    if offset > worker.size {
        return DownloadWorkerResult::Failed("partial_larger_than_remote".to_string());
    }

    let urls = match worker.client.locate_download_urls(
        worker.remote_path.as_str(),
        worker.account_uid,
        worker.account_bduss.as_str(),
    ) {
        Ok(urls) => urls,
        Err(err) => return DownloadWorkerResult::Failed(pcs_error_label(&err)),
    };
    let started_at = Instant::now();
    let mut file = match OpenOptions::new().create(true).append(true).open(&partial) {
        Ok(file) => file,
        Err(err) => return DownloadWorkerResult::Failed(io_error_label(&err)),
    };
    if offset > 0 {
        let _ = worker.tx.send(TransferTaskEvent::Progress {
            id: worker.id,
            bytes_done: offset,
            elapsed_seconds: 1,
        });
    }

    while offset < worker.size {
        if worker.cancellation.load(Ordering::Acquire) {
            return DownloadWorkerResult::Cancelled("user cancelled".to_string());
        }
        let end = offset
            .saturating_add(DOWNLOAD_CHUNK_BYTES)
            .saturating_sub(1)
            .min(worker.size.saturating_sub(1));
        let range = match baidupcsr_pcs::ByteRange::new(offset, end) {
            Ok(range) => range,
            Err(err) => return DownloadWorkerResult::Failed(pcs_error_label(&err)),
        };
        let bytes = match fetch_first_successful_dlink_range(&urls, |dlink| {
            worker
                .client
                .download_url_range_with_cancel(dlink, range, || {
                    worker.cancellation.load(Ordering::Acquire)
                })
        }) {
            DlinkRangeAttempt::Bytes(bytes) => bytes,
            DlinkRangeAttempt::Cancelled => {
                return DownloadWorkerResult::Cancelled("user cancelled".to_string());
            }
            DlinkRangeAttempt::Failed(message) => return DownloadWorkerResult::Failed(message),
        };
        if bytes.is_empty() {
            return DownloadWorkerResult::Failed("empty_range".to_string());
        }
        let allowed =
            usize::try_from(end.saturating_sub(offset).saturating_add(1)).unwrap_or(usize::MAX);
        let write_len = bytes.len().min(allowed);
        if let Err(err) = file.write_all(&bytes[..write_len]) {
            return DownloadWorkerResult::Failed(io_error_label(&err));
        }
        offset = offset.saturating_add(u64::try_from(write_len).unwrap_or(u64::MAX));
        let _ = worker.tx.send(TransferTaskEvent::Progress {
            id: worker.id,
            bytes_done: offset,
            elapsed_seconds: started_at.elapsed().as_secs().max(1),
        });
    }

    if let Err(err) = file.flush() {
        return DownloadWorkerResult::Failed(io_error_label(&err));
    }
    drop(file);
    match fs::rename(&partial, &worker.target) {
        Ok(()) => DownloadWorkerResult::Completed,
        Err(err) => DownloadWorkerResult::Failed(io_error_label(&err)),
    }
}

fn fetch_first_successful_dlink_range<F>(urls: &[String], mut fetch: F) -> DlinkRangeAttempt
where
    F: FnMut(&str) -> Result<Vec<u8>, baidupcsr_pcs::PcsError>,
{
    let mut last_error = "locatedownload_empty".to_string();
    for url in urls {
        match fetch(url.as_str()) {
            Ok(bytes) => return DlinkRangeAttempt::Bytes(bytes),
            Err(baidupcsr_pcs::PcsError::Cancelled { .. }) => return DlinkRangeAttempt::Cancelled,
            Err(err) => last_error = pcs_error_label(&err),
        }
    }
    DlinkRangeAttempt::Failed(last_error)
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

fn mpv_profile_from_config(
    profile: baidupcsr_config::VideoPlayerProfile,
) -> baidupcsr_preview::MpvLaunchProfile {
    match profile {
        baidupcsr_config::VideoPlayerProfile::User => baidupcsr_preview::MpvLaunchProfile::User,
        baidupcsr_config::VideoPlayerProfile::Isolated => {
            baidupcsr_preview::MpvLaunchProfile::Isolated
        }
    }
}

fn mpv_profile_label(profile: baidupcsr_config::VideoPlayerProfile) -> &'static str {
    match profile {
        baidupcsr_config::VideoPlayerProfile::User => "user",
        baidupcsr_config::VideoPlayerProfile::Isolated => "isolated",
    }
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

fn image_preview_lines(metadata: baidupcsr_preview::ImagePreviewMetadata) -> Vec<String> {
    let format = match metadata.format {
        baidupcsr_preview::ImagePreviewFormat::Png => "png",
        baidupcsr_preview::ImagePreviewFormat::Jpeg => "jpeg",
        baidupcsr_preview::ImagePreviewFormat::Unknown => "unknown",
    };
    let dimensions = match (metadata.width, metadata.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "unknown".to_string(),
    };
    vec![
        format!("image={format}"),
        format!("dimensions={dimensions}"),
        "terminal=metadata".to_string(),
        format!("disk={}", metadata.disk_backed),
    ]
}

fn program_available(program: &str) -> bool {
    if program.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(program).is_file();
    }
    env::var_os("PATH")
        .map(|paths| {
            env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(program);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

fn download_target_path(download_dir: &Path, file_name: &str) -> PathBuf {
    download_dir.join(sanitize_download_file_name(file_name))
}

fn partial_download_path(target: &Path) -> PathBuf {
    let mut file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("download")
        .to_string();
    file_name.push_str(".part");
    target.with_file_name(file_name)
}

fn selected_path_for_refresh(app: &baidupcsr_tui::TuiApp) -> Option<String> {
    app.selected_entry().map(|entry| entry.path.clone())
}

fn parse_remote_command(
    input: &str,
    app: &baidupcsr_tui::TuiApp,
) -> Result<ParsedRemoteCommand, String> {
    let mut parts = input.split_whitespace();
    let Some(command) = parts.next() else {
        return Err("empty command".to_string());
    };
    let args: Vec<&str> = parts.collect();

    match command {
        "mkdir" => {
            let path = args
                .first()
                .ok_or_else(|| "mkdir requires a path".to_string())?;
            Ok(ParsedRemoteCommand::Operation(
                baidupcsr_core::RemoteOperationPlan::mkdir(resolve_remote_path(app.path(), path))?,
            ))
        }
        "rename" | "ren" => {
            let target_name = args
                .first()
                .ok_or_else(|| "rename requires a new name".to_string())?;
            let (from, to) = if args.len() >= 2 {
                (
                    resolve_remote_path(app.path(), target_name),
                    resolve_remote_path(app.path(), args[1]),
                )
            } else {
                let entry = app
                    .selected_entry()
                    .ok_or_else(|| "rename requires a selected entry".to_string())?;
                let to = if target_name.starts_with('/') {
                    normalize_remote_command_path(target_name)
                } else {
                    join_remote_path(
                        remote_parent_path(entry.path.as_str()).as_str(),
                        target_name,
                    )
                };
                (entry.path.clone(), to)
            };
            Ok(ParsedRemoteCommand::Operation(
                baidupcsr_core::RemoteOperationPlan::rename(from, to)?,
            ))
        }
        "copy" | "cp" => {
            let target_dir = args
                .last()
                .ok_or_else(|| "copy requires a target directory".to_string())?;
            let pairs = command_target_paths_or_args(app, &args)?
                .into_iter()
                .map(|path| baidupcsr_core::RemotePathPair {
                    to: join_remote_path(
                        resolve_remote_path(app.path(), target_dir).as_str(),
                        remote_file_name(path.as_str()).as_str(),
                    ),
                    from: path,
                })
                .collect();
            Ok(ParsedRemoteCommand::Operation(
                baidupcsr_core::RemoteOperationPlan::copy(pairs)?,
            ))
        }
        "move" | "mv" => {
            let target_dir = args
                .last()
                .ok_or_else(|| "move requires a target directory".to_string())?;
            let pairs = command_target_paths_or_args(app, &args)?
                .into_iter()
                .map(|path| baidupcsr_core::RemotePathPair {
                    to: join_remote_path(
                        resolve_remote_path(app.path(), target_dir).as_str(),
                        remote_file_name(path.as_str()).as_str(),
                    ),
                    from: path,
                })
                .collect();
            Ok(ParsedRemoteCommand::Operation(
                baidupcsr_core::RemoteOperationPlan::move_paths(pairs)?,
            ))
        }
        "rm" | "delete" => Ok(ParsedRemoteCommand::Operation(
            baidupcsr_core::RemoteOperationPlan::delete(if args.is_empty() {
                command_target_paths(app)?
            } else {
                args.iter()
                    .map(|path| resolve_remote_path(app.path(), path))
                    .collect()
            })?,
        )),
        "search" => {
            let mut recursive = false;
            let mut words = Vec::new();
            for arg in args {
                if arg == "-r" {
                    recursive = true;
                } else {
                    words.push(arg);
                }
            }
            let keyword = words.join(" ");
            if keyword.is_empty() {
                return Err("search requires a keyword".to_string());
            }
            Ok(ParsedRemoteCommand::Search {
                path: app.path().to_string(),
                keyword,
                recursive,
            })
        }
        _ => Err(format!("unknown command: {command}")),
    }
}

fn command_target_paths(app: &baidupcsr_tui::TuiApp) -> Result<Vec<String>, String> {
    if !app.selected_paths().is_empty() {
        return Ok(app.selected_paths().to_vec());
    }
    app.selected_entry()
        .map(|entry| vec![entry.path.clone()])
        .ok_or_else(|| "command requires a selected entry".to_string())
}

fn command_target_paths_or_args(
    app: &baidupcsr_tui::TuiApp,
    args: &[&str],
) -> Result<Vec<String>, String> {
    if args.len() <= 1 {
        return command_target_paths(app);
    }
    Ok(args[..args.len() - 1]
        .iter()
        .map(|path| resolve_remote_path(app.path(), path))
        .collect())
}

fn execute_remote_operation(
    client: &baidupcsr_pcs::BlockingPcsClient,
    plan: &baidupcsr_core::RemoteOperationPlan,
) -> Result<(), String> {
    match plan.kind() {
        baidupcsr_core::RemoteOperationKind::Mkdir { path } => {
            client.mkdir(path).map_err(|err| pcs_error_label(&err))
        }
        baidupcsr_core::RemoteOperationKind::Delete { paths } => client
            .delete_paths(paths)
            .map_err(|err| pcs_error_label(&err)),
        baidupcsr_core::RemoteOperationKind::Rename { from, to } => {
            let pair = baidupcsr_pcs::FileOperationPair {
                from: from.clone(),
                to: to.clone(),
            };
            client
                .move_paths(&[pair])
                .map_err(|err| pcs_error_label(&err))
        }
        baidupcsr_core::RemoteOperationKind::Copy { pairs } => {
            let pairs = pcs_operation_pairs(pairs);
            client
                .copy_paths(&pairs)
                .map_err(|err| pcs_error_label(&err))
        }
        baidupcsr_core::RemoteOperationKind::Move { pairs } => {
            let pairs = pcs_operation_pairs(pairs);
            client
                .move_paths(&pairs)
                .map_err(|err| pcs_error_label(&err))
        }
    }
}

fn pcs_operation_pairs(
    pairs: &[baidupcsr_core::RemotePathPair],
) -> Vec<baidupcsr_pcs::FileOperationPair> {
    pairs
        .iter()
        .map(|pair| baidupcsr_pcs::FileOperationPair {
            from: pair.from.clone(),
            to: pair.to.clone(),
        })
        .collect()
}

fn resolve_remote_path(current_dir: &str, path: &str) -> String {
    if path.starts_with('/') {
        normalize_remote_command_path(path)
    } else {
        join_remote_path(current_dir, path)
    }
}

fn join_remote_path(dir: &str, name: &str) -> String {
    let dir = normalize_remote_command_path(dir);
    let name = name.trim_matches('/');
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn normalize_remote_command_path(path: &str) -> String {
    let mut path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    path
}

fn remote_parent_path(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }
    match path.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(index) => path[..index].to_string(),
    }
}

fn remote_file_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("remote")
        .to_string()
}

fn sanitize_download_file_name(file_name: &str) -> String {
    let sanitized: String = file_name
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "download".to_string()
    } else {
        sanitized
    }
}

fn task_summary_line(task: &baidupcsr_core::TaskSnapshot) -> String {
    let (kind, local_name) = match &task.kind {
        baidupcsr_core::TaskKind::Download { local_path, .. } => {
            ("download", file_name_label(local_path))
        }
        baidupcsr_core::TaskKind::Upload { local_path, .. } => {
            ("upload", file_name_label(local_path))
        }
        baidupcsr_core::TaskKind::RemoteOperation { label, .. } => ("remote", label.to_string()),
    };
    let total = task
        .progress
        .bytes_total
        .map(|value| value.to_string())
        .unwrap_or_else(|| "?".to_string());
    let mut line = format!(
        "#{} {} {} {}/{} {}",
        task.id.value(),
        kind,
        task_state_label(task.state),
        task.progress.bytes_done,
        total,
        local_name
    );
    if let Some(error) = &task.error {
        line.push(' ');
        line.push_str(error);
    }
    line
}

fn file_name_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn task_state_label(state: baidupcsr_core::TaskState) -> &'static str {
    match state {
        baidupcsr_core::TaskState::Queued => "queued",
        baidupcsr_core::TaskState::Running => "running",
        baidupcsr_core::TaskState::CancelRequested => "cancelling",
        baidupcsr_core::TaskState::Completed => "done",
        baidupcsr_core::TaskState::Failed => "failed",
        baidupcsr_core::TaskState::Cancelled => "cancelled",
    }
}

fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
        baidupcsr_preview::PreviewKind::Pdf => baidupcsr_tui::PreviewKind::Pdf,
        baidupcsr_preview::PreviewKind::Archive => baidupcsr_tui::PreviewKind::Archive,
        baidupcsr_preview::PreviewKind::Directory => baidupcsr_tui::PreviewKind::Directory,
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

fn download_dir_for_config(config: &baidupcsr_config::PcsConfig) -> PathBuf {
    if let Some(path) = env::var_os(ENV_DOWNLOAD_DIR).map(PathBuf::from) {
        return path;
    }
    if config.savedir.is_empty() {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(config.savedir.as_str())
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

fn pcs_error_label(err: &baidupcsr_pcs::PcsError) -> String {
    match err {
        baidupcsr_pcs::PcsError::Cancelled { .. } => "cancelled".to_string(),
        baidupcsr_pcs::PcsError::Parse { .. } => "parse".to_string(),
        baidupcsr_pcs::PcsError::Network { .. } => "network".to_string(),
        baidupcsr_pcs::PcsError::Remote { code, .. } => format!("remote_code={code}"),
    }
}

fn io_error_label(err: &std::io::Error) -> String {
    format!("io:{:?}", err.kind())
}

fn print_doctor(config_dir: Option<PathBuf>) {
    println!("baidupcsr doctor");
    println!("redacted: true");
    let discovery = baidupcsr_config::discovery_input_from_env(config_dir);
    let config_path = baidupcsr_config::discover_config_file(&discovery);
    let ui_config_path = baidupcsr_config::discover_ui_config_file();
    let cache_dir = baidupcsr_config::discover_cache_dir();
    println!("config_path: {}", config_path.display());
    println!("ui_config_path: {}", ui_config_path.display());
    println!("cache_dir: {}", cache_dir.display());
    println!(
        "config_permission: {}",
        config_permission_label(&config_path)
    );
    match baidupcsr_config::load_ui_config_or_default(&ui_config_path) {
        Ok(config) => {
            let state = if config.video_preview.enabled {
                "enabled"
            } else {
                "disabled"
            };
            println!(
                "video_preview: {state} profile={} chunk={} chunks={} budget={}",
                mpv_profile_label(config.video_preview.player_profile),
                config.video_preview.chunk_size_bytes,
                config.video_preview.max_chunks,
                video_memory_limit_from_config(&config.video_preview)
            );
        }
        Err(err) => {
            println!("video_preview: unknown");
            println!("ui_config_error: {err}");
        }
    }
    println!(
        "mpv: {}",
        if program_available("mpv") {
            "present"
        } else {
            "missing"
        }
    );

    match baidupcsr_config::load_config(&config_path) {
        Ok(config) => print_loaded_config(&config),
        Err(err) => {
            println!("config: not loaded");
            println!("config_error: {err}");
            println!(
                "repair: set BAIDUPCS_GO_CONFIG_DIR to your BaiduPCS-Go config directory or run BaiduPCS-Go login first"
            );
        }
    }

    println!("crates:");
    println!("  - {}", baidupcsr_config::crate_name());
    println!("  - {}", baidupcsr_core::crate_name());
    println!("  - {}", baidupcsr_pcs::crate_name());
    println!("  - {}", baidupcsr_preview::crate_name());
    println!("  - {}", baidupcsr_tui::crate_name());
}

fn config_permission_label(path: &Path) -> String {
    match fs::metadata(path) {
        Ok(metadata) => config_metadata_permission_label(&metadata),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(err) => io_error_label(&err),
    }
}

#[cfg(unix)]
fn config_metadata_permission_label(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode() & 0o777;
    if metadata.permissions().readonly() {
        format!("readonly mode={mode:03o}")
    } else if mode & 0o077 == 0 {
        format!("owner-only mode={mode:03o}")
    } else {
        format!("too-open mode={mode:03o}")
    }
}

#[cfg(not(unix))]
fn config_metadata_permission_label(metadata: &fs::Metadata) -> String {
    if metadata.permissions().readonly() {
        "readonly".to_string()
    } else {
        "present".to_string()
    }
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
        bduss_from_user, build_pcs_client, download_target_path,
        fetch_first_successful_dlink_range, is_video_entry, list_remote_directory,
        pan_user_agent_for_config, parse_remote_command, partial_download_path,
        preview_key_for_entry, selected_path_for_refresh, task_summary_line, tui_preview_kind,
        video_budget_from_config, DlinkRangeAttempt, ParsedRemoteCommand,
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

    fn expect_operation(
        command: ParsedRemoteCommand,
    ) -> Result<baidupcsr_core::RemoteOperationPlan, String> {
        match command {
            ParsedRemoteCommand::Operation(plan) => Ok(plan),
            ParsedRemoteCommand::Search { .. } => {
                Err("command parsed as search, expected operation".to_string())
            }
        }
    }

    fn expect_search(command: ParsedRemoteCommand) -> Result<(String, String, bool), String> {
        match command {
            ParsedRemoteCommand::Search {
                path,
                keyword,
                recursive,
            } => Ok((path, keyword, recursive)),
            ParsedRemoteCommand::Operation(_) => {
                Err("command parsed as operation, expected search".to_string())
            }
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
    fn download_target_uses_sanitized_filename_under_download_dir() {
        let target = download_target_path(Path::new("/tmp/downloads"), "../bad/name?.txt");

        assert_eq!(target, Path::new("/tmp/downloads").join(".._bad_name?.txt"));
        assert_eq!(
            partial_download_path(&target),
            Path::new("/tmp/downloads").join(".._bad_name?.txt.part")
        );
    }

    #[test]
    fn download_target_rejects_dot_path_components_as_filenames() {
        assert_eq!(
            download_target_path(Path::new("/tmp/downloads"), "."),
            Path::new("/tmp/downloads").join("download")
        );
        assert_eq!(
            download_target_path(Path::new("/tmp/downloads"), ".."),
            Path::new("/tmp/downloads").join("download")
        );
    }

    #[test]
    fn task_summary_uses_local_name_without_remote_path() -> Result<(), baidupcsr_core::TaskError> {
        let mut manager = baidupcsr_core::TaskManager::new();
        let id = manager.enqueue_download("/private/remote/secret.txt", "/tmp/out.txt", 1024);
        manager.start(id)?;
        manager.update_progress(id, 512, 1)?;
        let task = manager.snapshots().remove(0);

        let line = task_summary_line(&task);

        assert!(line.contains("#1 download running 512/1024 out.txt"));
        assert!(!line.contains("/private/remote"));
        Ok(())
    }

    #[test]
    fn selected_path_for_refresh_uses_current_selection() {
        let mut app = baidupcsr_tui::TuiApp::new("/docs");
        app.set_entries(vec![
            file(1, 12),
            FileEntry {
                fs_id: 2,
                path: "/docs/second.md".to_string(),
                server_filename: "second.md".to_string(),
                ..file(2, 34)
            },
        ]);
        app.handle_key(baidupcsr_tui::KeyEvent::Char('j'));

        assert_eq!(
            selected_path_for_refresh(&app),
            Some("/docs/second.md".to_string())
        );
    }

    #[test]
    fn remote_command_mkdir_resolves_relative_path_under_current_directory() -> Result<(), String> {
        let app = baidupcsr_tui::TuiApp::new("/docs");

        let plan = expect_operation(parse_remote_command("mkdir reports", &app)?)?;

        assert_eq!(plan.dry_run_lines()[0], "mkdir /docs/reports");
        Ok(())
    }

    #[test]
    fn remote_command_rename_targets_selected_entry_parent() -> Result<(), String> {
        let mut app = baidupcsr_tui::TuiApp::new("/docs");
        app.set_entries(vec![file(1, 12)]);

        let plan = expect_operation(parse_remote_command("rename renamed.md", &app)?)?;

        assert_eq!(
            plan.dry_run_lines()[0],
            "rename /docs/notes.md -> /docs/renamed.md"
        );
        Ok(())
    }

    #[test]
    fn remote_command_rename_accepts_explicit_from_to() -> Result<(), String> {
        let app = baidupcsr_tui::TuiApp::new("/docs");

        let plan = expect_operation(parse_remote_command("rename old.md new.md", &app)?)?;

        assert_eq!(
            plan.dry_run_lines()[0],
            "rename /docs/old.md -> /docs/new.md"
        );
        Ok(())
    }

    #[test]
    fn remote_command_copy_uses_multi_selection_when_present() -> Result<(), String> {
        let mut app = baidupcsr_tui::TuiApp::new("/docs");
        app.set_entries(vec![
            file(1, 12),
            FileEntry {
                fs_id: 2,
                path: "/docs/second.md".to_string(),
                server_filename: "second.md".to_string(),
                ..file(2, 34)
            },
        ]);
        app.handle_key_event(baidupcsr_tui::KeyEvent::Char(' '));
        app.handle_key_event(baidupcsr_tui::KeyEvent::Char('j'));
        app.handle_key_event(baidupcsr_tui::KeyEvent::Char(' '));

        let plan = expect_operation(parse_remote_command("copy /backup", &app)?)?;

        assert_eq!(
            plan.dry_run_lines(),
            &[
                "copy 2 remote entries".to_string(),
                "/docs/notes.md -> /backup/notes.md".to_string(),
                "/docs/second.md -> /backup/second.md".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn remote_command_delete_requires_operation_plan_from_selection() -> Result<(), String> {
        let mut app = baidupcsr_tui::TuiApp::new("/docs");
        app.set_entries(vec![file(1, 12)]);

        let plan = expect_operation(parse_remote_command("rm", &app)?)?;

        assert!(plan.is_destructive());
        assert_eq!(plan.dry_run_lines()[0], "delete 1 remote entries");
        Ok(())
    }

    #[test]
    fn remote_command_delete_accepts_explicit_paths() -> Result<(), String> {
        let app = baidupcsr_tui::TuiApp::new("/docs");

        let plan = expect_operation(parse_remote_command("rm a.md nested/b.md", &app)?)?;

        assert_eq!(
            plan.dry_run_lines(),
            &[
                "delete 2 remote entries".to_string(),
                "/docs/a.md".to_string(),
                "/docs/nested/b.md".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn remote_command_move_accepts_explicit_sources_and_target_directory() -> Result<(), String> {
        let app = baidupcsr_tui::TuiApp::new("/docs");

        let plan = expect_operation(parse_remote_command("move a.md b.md /archive", &app)?)?;

        assert_eq!(
            plan.dry_run_lines(),
            &[
                "move 2 remote entries".to_string(),
                "/docs/a.md -> /archive/a.md".to_string(),
                "/docs/b.md -> /archive/b.md".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn remote_command_search_parses_recursive_flag() -> Result<(), String> {
        let app = baidupcsr_tui::TuiApp::new("/docs");

        let (path, keyword, recursive) =
            expect_search(parse_remote_command("search -r notes", &app)?)?;

        assert_eq!(path, "/docs");
        assert_eq!(keyword, "notes");
        assert!(recursive);
        Ok(())
    }

    #[test]
    fn dlink_range_fetch_tries_later_candidates_after_network_failure() {
        let urls = vec![
            "https://bad.invalid/file".to_string(),
            "https://good.invalid/file".to_string(),
        ];
        let mut attempted = Vec::new();

        let result = fetch_first_successful_dlink_range(&urls, |url| {
            attempted.push(url.to_string());
            if url.contains("bad") {
                Err(baidupcsr_pcs::PcsError::Network {
                    operation: "download".to_string(),
                    message: "redacted".to_string(),
                })
            } else {
                Ok(vec![1, 2, 3])
            }
        });

        assert_eq!(result, DlinkRangeAttempt::Bytes(vec![1, 2, 3]));
        assert_eq!(attempted, urls);
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
    fn preview_kind_mapping_preserves_all_preview_boundaries() {
        assert_eq!(
            tui_preview_kind(baidupcsr_preview::PreviewKind::Text),
            baidupcsr_tui::PreviewKind::Text
        );
        assert_eq!(
            tui_preview_kind(baidupcsr_preview::PreviewKind::Image),
            baidupcsr_tui::PreviewKind::Image
        );
        assert_eq!(
            tui_preview_kind(baidupcsr_preview::PreviewKind::Pdf),
            baidupcsr_tui::PreviewKind::Pdf
        );
        assert_eq!(
            tui_preview_kind(baidupcsr_preview::PreviewKind::Archive),
            baidupcsr_tui::PreviewKind::Archive
        );
        assert_eq!(
            tui_preview_kind(baidupcsr_preview::PreviewKind::Directory),
            baidupcsr_tui::PreviewKind::Directory
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
            player_profile: baidupcsr_config::VideoPlayerProfile::Isolated,
        };
        let budget = video_budget_from_config(&config)?;

        assert_eq!(budget.chunk_size_bytes, 128 * 1024);
        assert_eq!(budget.max_chunks, 3);
        assert_eq!(budget.memory_limit_bytes(), 384 * 1024);
        Ok(())
    }
}
