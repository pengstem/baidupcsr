use super::*;

const USAGE: &str = "usage: baidupcsr-cli [tui|doctor [--redact]|video-bench --path REMOTE_PATH [--case source|gateway|all] [--duration SECONDS] [--output PATH] [--redact]]";

pub async fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args: Vec<String> = args
        .into_iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    match parse_command(&args) {
        CommandAction::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        CommandAction::Doctor { config_dir } => {
            print_doctor(config_dir);
            ExitCode::SUCCESS
        }
        CommandAction::VideoBench(options) => match run_video_bench(&options).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("video_bench_error: {err}");
                ExitCode::from(1)
            }
        },
        CommandAction::TuiShell => {
            let mut runtime = BrowserRuntime::from_config();
            let app = runtime.starting_app();
            let root = runtime.root.clone();
            let result = {
                let mut handler = |app: &mut crate::tui::TuiApp, event| {
                    runtime.handle_event(app, event);
                };
                crate::tui::run_terminal_app_with_handler(app, root, &mut handler).await
            };
            runtime.shutdown().await;
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("tui_error: {err}");
                    ExitCode::from(1)
                }
            }
        }
        CommandAction::UsageError => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

pub(crate) struct BrowserRuntime {
    pub(crate) client: Option<crate::pcs::PcsClient>,
    pub(crate) account_uid: u64,
    pub(crate) account_bduss: Option<String>,
    pub(crate) config_path: PathBuf,
    pub(crate) cache: crate::app::DirectoryCache,
    pub(crate) cache_store: crate::app::DirectoryCacheStore,
    pub(crate) loaded_at: HashMap<String, SystemTime>,
    pub(crate) cache_sources: HashMap<String, crate::tui::CacheSource>,
    pub(crate) selection_memory: HashMap<String, String>,
    pub(crate) preload_planner: crate::app::PagedPreloadPlanner,
    pub(crate) preview_manager: crate::preview::PreviewManager,
    pub(crate) preview_epoch: Arc<AtomicU64>,
    pub(crate) preview_cancellation: Option<tokio_util::sync::CancellationToken>,
    pub(crate) video_config: crate::config::VideoPreviewConfig,
    pub(crate) video_epoch: Arc<AtomicU64>,
    pub(crate) active_video: Option<ActiveVideoPreview>,
    pub(crate) download_dir: PathBuf,
    pub(crate) task_manager: crate::app::TaskManager,
    pub(crate) task_cancellations: HashMap<crate::app::TaskId, tokio_util::sync::CancellationToken>,
    pub(crate) pending_remote_operation: Option<crate::app::RemoteOperationPlan>,
    pub(crate) tx: mpsc::Sender<DirectoryLoadResult>,
    pub(crate) rx: mpsc::Receiver<DirectoryLoadResult>,
    pub(crate) preview_tx: mpsc::Sender<PreviewLoadResult>,
    pub(crate) preview_rx: mpsc::Receiver<PreviewLoadResult>,
    pub(crate) task_tx: mpsc::Sender<TransferTaskEvent>,
    pub(crate) task_rx: mpsc::Receiver<TransferTaskEvent>,
    pub(crate) root: tokio_util::sync::CancellationToken,
    pub(crate) tasks: tokio::task::JoinSet<()>,
    pub(crate) directory_cancellation: Option<tokio_util::sync::CancellationToken>,
    pub(crate) inflight: usize,
    pub(crate) initial_path: String,
    pub(crate) current_path: String,
    pub(crate) startup_notice: Option<String>,
    pub(crate) shutdown_complete: bool,
}

pub(crate) struct DirectoryLoadResult {
    pub(crate) generation: crate::app::RefreshGeneration,
    pub(crate) path: String,
    pub(crate) result: Result<Vec<crate::pcs::FileEntry>, crate::pcs::PcsError>,
}

pub(crate) struct PreviewLoadResult {
    pub(crate) request: crate::preview::PreviewRequest,
    pub(crate) kind: PreviewFetchKind,
    pub(crate) title: String,
    pub(crate) result: Result<Vec<u8>, crate::pcs::PcsError>,
}

pub(crate) struct PreviewLoadJob {
    pub(crate) client: crate::pcs::PcsClient,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) request: crate::preview::PreviewRequest,
    pub(crate) kind: PreviewFetchKind,
    pub(crate) fetch_bytes: u64,
    pub(crate) preview_epoch: u64,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreviewFetchKind {
    Text,
    ImageMetadata,
}

pub(crate) struct ActiveVideoPreview {
    pub(crate) key: crate::preview::PreviewKey,
    pub(crate) title: String,
    pub(crate) server: crate::preview::VideoLoopbackServer,
    pub(crate) player: tokio::process::Child,
    pub(crate) player_log: Arc<Mutex<crate::preview::PlayerLogRing>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferWorkerMode {
    Download,
    LocalOpen,
}

pub(crate) enum TransferTaskEvent {
    Progress {
        id: crate::app::TaskId,
        bytes_done: u64,
        elapsed_seconds: u64,
    },
    Completed {
        id: crate::app::TaskId,
        mode: TransferWorkerMode,
    },
    Failed {
        id: crate::app::TaskId,
        mode: TransferWorkerMode,
        message: String,
    },
    Cancelled {
        id: crate::app::TaskId,
        mode: TransferWorkerMode,
    },
    RemoteCompleted {
        id: crate::app::TaskId,
        label: String,
        affected_dirs: Vec<String>,
    },
    RemoteFailed {
        id: crate::app::TaskId,
        message: String,
    },
    SearchBatch {
        id: crate::app::TaskId,
        path: String,
        entries: Vec<crate::pcs::FileEntry>,
        scanned_dirs: usize,
        scanned_entries: usize,
    },
    SearchCompleted {
        id: crate::app::TaskId,
        path: String,
        keyword: String,
        result: Result<RemoteSearchSummary, String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ParsedRemoteCommand {
    Operation(crate::app::RemoteOperationPlan),
    Search {
        path: String,
        keyword: String,
        mode: RemoteSearchMode,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteSearchMode {
    RecursiveList,
    CurrentList,
    PcsApi,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RemoteSearchSummary {
    pub(crate) scanned_dirs: usize,
    pub(crate) scanned_entries: usize,
    pub(crate) matches: usize,
}

impl Drop for BrowserRuntime {
    fn drop(&mut self) {
        self.root.cancel();
        self.tasks.abort_all();
        self.stop_active_video();
        if !self.shutdown_complete {
            if let Err(err) = self.persist_current_workdir() {
                eprintln!("workdir_persist_error: {err}");
            }
        }
    }
}

impl BrowserRuntime {
    pub(crate) fn from_config() -> Self {
        let (tx, rx) = mpsc::channel(APP_EVENT_CAPACITY);
        let (preview_tx, preview_rx) = mpsc::channel(APP_EVENT_CAPACITY);
        let (task_tx, task_rx) = mpsc::channel(APP_EVENT_CAPACITY);
        let root = tokio_util::sync::CancellationToken::new();
        let tasks = tokio::task::JoinSet::new();
        let ui_config_path = crate::config::discover_ui_config_file();
        let ui_config = crate::config::load_ui_config_or_default(&ui_config_path)
            .unwrap_or_else(|_| crate::config::UiConfig::default());
        let cache_store = crate::app::DirectoryCacheStore::new(crate::config::discover_cache_dir());
        let preload_planner = crate::app::PagedPreloadPlanner::new(18, 1, 4, 4);
        let config_path = crate::config::discover_config_file(None);

        let mut client: Option<crate::pcs::PcsClient> = None;
        let mut account_uid: u64 = 0;
        let mut account_bduss: Option<String> = None;
        let mut download_dir = PathBuf::from(".");
        let mut initial_path = "/".to_string();
        let mut startup_notice: Option<String> = None;

        match crate::config::load_config(&config_path) {
            Ok(loaded) => {
                if let Some(user) = loaded.active_user() {
                    initial_path = if user.workdir.is_empty() {
                        "/".to_string()
                    } else {
                        user.workdir.clone()
                    };
                    download_dir = download_dir_for_config(&loaded);
                    account_uid = user.uid;
                    account_bduss = bduss_from_user(user);
                    match build_pcs_client(&loaded, user) {
                        Ok(c) => client = Some(c),
                        Err(_) => startup_notice = Some("auth_error".to_string()),
                    }
                } else {
                    download_dir = download_dir_for_config(&loaded);
                    startup_notice = Some("active_user: missing".to_string());
                }
            }
            Err(err) => {
                startup_notice = Some(format!("config_error: {err}"));
            }
        }

        Self {
            client,
            account_uid,
            account_bduss,
            config_path,
            cache: crate::app::DirectoryCache::new(),
            cache_store,
            loaded_at: HashMap::new(),
            cache_sources: HashMap::new(),
            selection_memory: HashMap::new(),
            preload_planner,
            preview_manager: crate::preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
            preview_epoch: Arc::new(AtomicU64::new(0)),
            preview_cancellation: None,
            video_config: ui_config.video_preview,
            video_epoch: Arc::new(AtomicU64::new(0)),
            active_video: None,
            download_dir,
            task_manager: crate::app::TaskManager::new(),
            task_cancellations: HashMap::new(),
            pending_remote_operation: None,
            tx,
            rx,
            preview_tx,
            preview_rx,
            task_tx,
            task_rx,
            root,
            tasks,
            directory_cancellation: None,
            inflight: 0,
            current_path: initial_path.clone(),
            initial_path,
            startup_notice,
            shutdown_complete: false,
        }
    }

    pub(crate) fn starting_app(&mut self) -> crate::tui::TuiApp {
        let mut app = crate::tui::TuiApp::new(&self.initial_path);
        if let Some(notice) = self.startup_notice.clone() {
            app.set_notice(notice);
        } else {
            self.start_directory_load(&mut app, self.initial_path.clone(), None);
        }
        app
    }

    pub(crate) async fn shutdown(&mut self) {
        self.root.cancel();
        if let Some(cancel) = self.directory_cancellation.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.preview_cancellation.take() {
            cancel.cancel();
        }
        for cancel in self.task_cancellations.values() {
            cancel.cancel();
        }
        self.shutdown_active_video().await;
        if let Err(err) = self.persist_current_workdir() {
            eprintln!("workdir_persist_error: {err}");
        }
        crate::tui::shutdown_tasks(&mut self.tasks, &self.root, Duration::from_secs(2)).await;
        self.shutdown_complete = true;
    }

    pub(crate) fn reap_finished_tasks(&mut self) {
        while self.tasks.try_join_next().is_some() {}
    }

    pub(crate) fn persist_current_workdir(&self) -> Result<(), crate::config::ConfigError> {
        persist_workdir_if_active(
            self.account_uid,
            &self.config_path,
            self.current_path.as_str(),
        )
    }

    pub(crate) fn handle_event(
        &mut self,
        app: &mut crate::tui::TuiApp,
        event: crate::tui::TuiRuntimeEvent,
    ) {
        match event {
            crate::tui::TuiRuntimeEvent::Tick => {
                self.reap_finished_tasks();
                self.drain_directory_results(app);
                self.drain_preview_results(app);
                self.drain_transfer_task_events(app);
                self.refresh_active_video_preview(app);
                self.refresh_current_cache_age(app);
                self.refresh_task_panel(app);
            }
            crate::tui::TuiRuntimeEvent::Action(action) => self.handle_action(app, action),
            crate::tui::TuiRuntimeEvent::CommandSubmitted(command) => {
                self.handle_remote_command(app, command.as_str());
            }
            crate::tui::TuiRuntimeEvent::SearchSubmitted(keyword) => {
                self.start_search_task(
                    app,
                    app.path().to_string(),
                    keyword,
                    RemoteSearchMode::RecursiveList,
                );
            }
            crate::tui::TuiRuntimeEvent::RenameSubmitted(name) => {
                self.start_rename_selected(app, name.as_str());
            }
            crate::tui::TuiRuntimeEvent::PathRequested(path) => {
                self.handle_path_request(app, path);
            }
            crate::tui::TuiRuntimeEvent::ConfirmAccepted => {
                self.confirm_pending_remote_operation(app);
            }
            crate::tui::TuiRuntimeEvent::ConfirmRejected => {
                self.pending_remote_operation = None;
                app.set_notice("operation_cancelled");
            }
        }
    }

    pub(crate) fn handle_action(
        &mut self,
        app: &mut crate::tui::TuiApp,
        action: crate::tui::KeyAction,
    ) {
        match action {
            crate::tui::KeyAction::Enter => {
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
            crate::tui::KeyAction::Back => {
                if let Some(path) = app.search_origin_path().map(str::to_string) {
                    app.exit_search_results();
                    self.stop_active_video();
                    self.start_directory_load(app, path, None);
                    return;
                }
                if let Some(path) = crate::tui::TuiApp::parent_path(app.path()) {
                    self.stop_active_video();
                    let current_path = app.path().to_string();
                    self.start_directory_load(app, path, Some(current_path.as_str()));
                }
            }
            crate::tui::KeyAction::Refresh => {
                let preferred = app.selected_entry().map(|entry| entry.path.clone());
                self.start_directory_load(app, app.path().to_string(), preferred.as_deref());
            }
            crate::tui::KeyAction::Delete => {
                self.confirm_delete_selection(app);
            }
            crate::tui::KeyAction::Up
            | crate::tui::KeyAction::Down
            | crate::tui::KeyAction::PageUp
            | crate::tui::KeyAction::PageDown
            | crate::tui::KeyAction::HalfPageUp
            | crate::tui::KeyAction::HalfPageDown
            | crate::tui::KeyAction::Top
            | crate::tui::KeyAction::Bottom
            | crate::tui::KeyAction::Select => {
                self.update_preview_for_selection(app);
                self.prefetch_visible_page(app);
            }
            crate::tui::KeyAction::Preview => {
                self.start_preview_action(app);
            }
            crate::tui::KeyAction::Download => {
                self.start_download_task(app);
            }
            crate::tui::KeyAction::CancelTask => {
                self.cancel_latest_task(app);
            }
            crate::tui::KeyAction::Filter
            | crate::tui::KeyAction::Search
            | crate::tui::KeyAction::Rename
            | crate::tui::KeyAction::Sort
            | crate::tui::KeyAction::Command
            | crate::tui::KeyAction::ToggleSelect
            | crate::tui::KeyAction::Quit => {}
        }
        self.refresh_task_panel(app);
    }

    pub(crate) fn confirm_delete_selection(&mut self, app: &mut crate::tui::TuiApp) {
        match delete_plan_for_selection(app) {
            Ok(plan) => self.begin_remote_operation_confirm(app, plan),
            Err(message) => app.set_notice(format!("delete_error: {message}")),
        }
    }

    pub(crate) fn start_rename_selected(
        &mut self,
        app: &mut crate::tui::TuiApp,
        target_name: &str,
    ) {
        match rename_plan_for_selected(app, target_name) {
            Ok(Some(plan)) => self.start_remote_operation_task(app, plan),
            Ok(None) => app.set_notice("rename_skipped: unchanged"),
            Err(message) => app.set_notice(format!("rename_error: {message}")),
        }
    }

    pub(crate) fn begin_remote_operation_confirm(
        &mut self,
        app: &mut crate::tui::TuiApp,
        plan: crate::app::RemoteOperationPlan,
    ) {
        self.pending_remote_operation = Some(plan.clone());
        let mut lines = plan.dry_run_lines().to_vec();
        lines.push("press y to confirm, n to cancel".to_string());
        app.begin_confirm(lines);
    }

    pub(crate) fn handle_path_request(&mut self, app: &mut crate::tui::TuiApp, path: String) {
        if path == app.path() {
            return;
        }
        let preferred = breadcrumb_preferred_selection(app.path(), path.as_str());
        self.start_directory_load(app, path, preferred.as_deref());
    }

    pub(crate) fn handle_remote_command(&mut self, app: &mut crate::tui::TuiApp, command: &str) {
        match parse_remote_command(command, app) {
            Ok(ParsedRemoteCommand::Search {
                path,
                keyword,
                mode,
            }) => self.start_search_task(app, path, keyword, mode),
            Ok(ParsedRemoteCommand::Operation(plan)) if plan.is_destructive() => {
                self.begin_remote_operation_confirm(app, plan);
            }
            Ok(ParsedRemoteCommand::Operation(plan)) => {
                self.start_remote_operation_task(app, plan);
            }
            Err(message) => app.set_notice(format!("command_error: {message}")),
        }
        self.refresh_task_panel(app);
    }

    pub(crate) fn confirm_pending_remote_operation(&mut self, app: &mut crate::tui::TuiApp) {
        let Some(plan) = self.pending_remote_operation.take() else {
            app.set_notice("operation_confirm: no pending operation");
            return;
        };
        self.start_remote_operation_task(app, plan);
    }

    pub(crate) fn start_remote_operation_task(
        &mut self,
        app: &mut crate::tui::TuiApp,
        plan: crate::app::RemoteOperationPlan,
    ) {
        let Some(client) = self.client.clone() else {
            app.set_notice("operation_error: client unavailable");
            return;
        };
        let label = plan.label().to_string();
        let affected_dirs = plan.affected_dirs().to_vec();
        let id = self.task_manager.enqueue_remote_operation(label.as_str());
        if self.task_manager.start(id).is_err() {
            app.set_notice("operation_error: failed to start task");
            return;
        }

        let tx = self.task_tx.clone();
        let notice_label = label.clone();
        let cancel = self.root.child_token();
        self.task_cancellations.insert(id, cancel.clone());
        self.tasks.spawn(async move {
            let event = match execute_remote_operation(&client, &plan, &cancel).await {
                Ok(()) => TransferTaskEvent::RemoteCompleted {
                    id,
                    label,
                    affected_dirs,
                },
                Err(message) => TransferTaskEvent::RemoteFailed { id, message },
            };
            let _ = tx.send(event).await;
        });
        app.set_notice(format!(
            "operation_started: #{} {}",
            id.value(),
            notice_label
        ));
    }

    pub(crate) fn start_search_task(
        &mut self,
        app: &mut crate::tui::TuiApp,
        path: String,
        keyword: String,
        mode: RemoteSearchMode,
    ) {
        let Some(client) = self.client.clone() else {
            app.set_notice("search_error: client unavailable");
            return;
        };
        let id = self.task_manager.enqueue_remote_operation("search");
        if self.task_manager.start(id).is_err() {
            app.set_notice("search_error: failed to start task");
            return;
        }

        let tx = self.task_tx.clone();
        app.begin_search_results(path.as_str(), keyword.as_str());
        app.set_notice(format!("search_started: #{} {}", id.value(), keyword));
        let cancel = self.root.child_token();
        self.task_cancellations.insert(id, cancel.clone());
        self.tasks.spawn(async move {
            let result =
                run_remote_search_task(client, tx.clone(), id, &path, &keyword, mode, &cancel)
                    .await
                    .map_err(|err| pcs_error_label(&err));
            let _ = tx
                .send(TransferTaskEvent::SearchCompleted {
                    id,
                    path,
                    keyword,
                    result,
                })
                .await;
        });
    }

    pub(crate) fn start_directory_load(
        &mut self,
        app: &mut crate::tui::TuiApp,
        path: String,
        preferred_selection_path: Option<&str>,
    ) {
        if let Some(cancel) = self.directory_cancellation.take() {
            cancel.cancel();
        }
        self.cancel_preview_load();
        self.stop_active_video();
        self.current_path.clone_from(&path);
        self.hydrate_directory_cache_from_disk(path.as_str());
        let view = self.cache.enter_directory(path.as_str());
        let cache_age = self.cache_age_seconds(path.as_str());
        app.set_cache_source(self.cache_source_for_path(path.as_str()));
        let preferred = preferred_selection_path
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

        let cancel = self.root.child_token();
        self.directory_cancellation = Some(cancel.clone());
        self.spawn_directory_load(client, path, view.generation, cancel);
        app.set_background_tasks(self.inflight);
        self.prefetch_visible_page(app);
    }

    pub(crate) fn prefetch_visible_page(&mut self, app: &mut crate::tui::TuiApp) {
        let selected = app.selected_index().unwrap_or(0);
        let plan = self.preload_planner.plan(app.entries(), selected);
        let directories = plan
            .targets
            .iter()
            .filter(|target| target.kind == crate::app::PreloadKind::Directory)
            .count();
        let previews = plan
            .targets
            .iter()
            .filter(|target| target.kind == crate::app::PreloadKind::Preview)
            .count();
        app.set_preload_lines(vec![
            format!("queued dirs={directories} previews={previews}"),
            format!("window={}..{}", plan.window_start, plan.window_end),
        ]);

        for target in plan.targets {
            match target.kind {
                crate::app::PreloadKind::Directory => {
                    self.prefetch_directory_path(app, target.path);
                }
                crate::app::PreloadKind::Preview => {
                    self.prefetch_preview_path(app, target.path.as_str());
                }
            }
        }
    }

    pub(crate) fn prefetch_directory_path(&mut self, app: &mut crate::tui::TuiApp, path: String) {
        self.hydrate_directory_cache_from_disk(path.as_str());
        let source = self.cache_source_for_path(path.as_str());
        let status = self.cache.snapshot(path.as_str()).status;
        if status == crate::app::DirectoryStatus::Refreshing {
            return;
        }
        if status == crate::app::DirectoryStatus::Fresh
            && source == crate::tui::CacheSource::Network
        {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let generation = self.cache.begin_refresh(path.as_str());
        let cancel = self.root.child_token();
        self.spawn_directory_load(client, path, generation, cancel);
        app.set_background_tasks(self.inflight);
    }

    pub(crate) fn prefetch_preview_path(&mut self, app: &crate::tui::TuiApp, path: &str) {
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
        let plan = crate::preview::PreviewPlan::for_entry(
            entry.server_filename.as_str(),
            entry.is_dir,
            size,
            PREVIEW_MEMORY_LIMIT_BYTES,
        );
        if plan.action != crate::preview::PreviewAction::FetchText {
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
        let cancel = self.root.child_token();
        self.spawn_preview_load(PreviewLoadJob {
            client,
            path: entry.path,
            title: entry.server_filename,
            request,
            kind: PreviewFetchKind::Text,
            fetch_bytes,
            preview_epoch,
            cancel,
        });
    }

    pub(crate) fn spawn_directory_load(
        &mut self,
        client: crate::pcs::PcsClient,
        path: String,
        generation: crate::app::RefreshGeneration,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        self.inflight = self.inflight.saturating_add(1);
        let tx = self.tx.clone();
        self.tasks.spawn(async move {
            let result = client.list(path.as_str(), &cancel).await;
            let _ = tx
                .send(DirectoryLoadResult {
                    generation,
                    path,
                    result,
                })
                .await;
        });
    }

    pub(crate) fn drain_directory_results(&mut self, app: &mut crate::tui::TuiApp) {
        while let Ok(result) = self.rx.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            let path = result.path;
            match result.result {
                Ok(entries) => {
                    let entries_for_store = entries.clone();
                    let applied =
                        self.cache
                            .finish_refresh(path.as_str(), result.generation, entries);
                    if applied {
                        self.loaded_at.insert(path.clone(), SystemTime::now());
                        self.cache_sources
                            .insert(path.clone(), crate::tui::CacheSource::Network);
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
                Err(error) => {
                    let applied = self
                        .cache
                        .finish_refresh_error(path.as_str(), result.generation);
                    if applied && app.path() == path {
                        let view = self.cache.snapshot(path.as_str());
                        let preferred =
                            self.selection_memory.get(path.as_str()).map(String::as_str);
                        app.finish_directory_error(
                            path.as_str(),
                            view.entries,
                            self.cache_age_seconds(path.as_str()),
                            format!("remote_list_error: {error}"),
                            preferred,
                        );
                        app.set_cache_source(self.cache_source_for_path(path.as_str()));
                    }
                }
            }
        }
        app.set_background_tasks(self.inflight);
    }

    pub(crate) fn update_preview_for_selection(&mut self, app: &mut crate::tui::TuiApp) {
        let selected = app.selected_entry().cloned();
        self.cancel_preview_load();
        self.preview_cancellation = selected.as_ref().map(|_| self.root.child_token());
        let preview_epoch = self
            .preview_epoch
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.stop_active_video_if_selection_changed(app);
        let Some(entry) = selected else {
            app.begin_preview("metadata", "no selected entry");
            return;
        };

        let size = nonnegative_size(entry.size);
        let plan = crate::preview::PreviewPlan::for_entry(
            entry.server_filename.as_str(),
            entry.is_dir,
            size,
            PREVIEW_MEMORY_LIMIT_BYTES,
        );
        let budget = crate::preview::PreviewStatus {
            kind: plan.kind,
            fetched_bytes: 0,
            memory_limit_bytes: PREVIEW_MEMORY_LIMIT_BYTES,
        };

        if plan.kind == crate::preview::PreviewKind::Video {
            let reason = if self.video_config.enabled {
                "press p for strict loopback video preview"
            } else {
                "video preview disabled by sidecar kill switch"
            };
            app.finish_preview_metadata(
                entry.server_filename.as_str(),
                crate::preview::PreviewKind::Video,
                reason,
                crate::preview::PreviewStatus {
                    kind: crate::preview::PreviewKind::Video,
                    ..budget
                },
            );
            return;
        }

        match plan.action {
            crate::preview::PreviewAction::MetadataOnly => {
                let reason = if entry.is_dir {
                    plan.reason
                } else {
                    format!("{}; press p to open local copy (<100MiB)", plan.reason)
                };
                app.finish_preview_metadata(
                    entry.server_filename.as_str(),
                    plan.kind,
                    reason.as_str(),
                    budget,
                );
            }
            crate::preview::PreviewAction::FetchText => {
                self.start_text_preview_load(app, entry, size, preview_epoch);
            }
            crate::preview::PreviewAction::FetchImageMetadata => {
                self.start_image_metadata_preview_load(app, entry, size, preview_epoch);
            }
        }
    }

    pub(crate) fn start_text_preview_load(
        &mut self,
        app: &mut crate::tui::TuiApp,
        entry: crate::pcs::FileEntry,
        size: u64,
        preview_epoch: u64,
    ) {
        let key = preview_key_for_entry(self.account_uid, &entry);
        if let Some(text) = self.preview_manager.get(&key) {
            let content = text.content.clone();
            let status = text.status;
            app.finish_preview_text(entry.server_filename.as_str(), content.as_str(), status);
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
            if let Some(status) = self.preview_manager.finish_text_request(request, b"") {
                app.finish_preview_text(entry.server_filename.as_str(), "", status);
            }
            return;
        }

        let cancel = self
            .preview_cancellation
            .as_ref()
            .cloned()
            .unwrap_or_else(|| self.root.child_token());
        self.spawn_preview_load(PreviewLoadJob {
            client,
            path: entry.path,
            title: entry.server_filename,
            request,
            kind: PreviewFetchKind::Text,
            fetch_bytes,
            preview_epoch,
            cancel,
        });
        app.set_background_tasks(self.inflight);
    }

    pub(crate) fn start_image_metadata_preview_load(
        &mut self,
        app: &mut crate::tui::TuiApp,
        entry: crate::pcs::FileEntry,
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
                crate::preview::PreviewStatus {
                    kind: crate::preview::PreviewKind::Image,
                    fetched_bytes: 0,
                    memory_limit_bytes: PREVIEW_MEMORY_LIMIT_BYTES,
                },
            );
            return;
        }

        let cancel = self
            .preview_cancellation
            .as_ref()
            .cloned()
            .unwrap_or_else(|| self.root.child_token());
        self.spawn_preview_load(PreviewLoadJob {
            client,
            path: entry.path,
            title: entry.server_filename,
            request,
            kind: PreviewFetchKind::ImageMetadata,
            fetch_bytes,
            preview_epoch,
            cancel,
        });
        app.set_background_tasks(self.inflight);
    }

    pub(crate) fn spawn_preview_load(&mut self, job: PreviewLoadJob) {
        self.inflight = self.inflight.saturating_add(1);
        let tx = self.preview_tx.clone();
        let cancellation_epoch = Arc::clone(&self.preview_epoch);
        self.tasks.spawn(async move {
            if cancellation_epoch.load(Ordering::Acquire) != job.preview_epoch {
                job.cancel.cancel();
            }
            let result = match crate::pcs::ByteRange::prefix(job.fetch_bytes) {
                Ok(range) => {
                    job.client
                        .download_range(job.path.as_str(), range, &job.cancel)
                        .await
                }
                Err(error) => Err(error),
            };
            let _ = tx
                .send(PreviewLoadResult {
                    request: job.request,
                    kind: job.kind,
                    title: job.title,
                    result,
                })
                .await;
        });
    }

    pub(crate) fn cancel_preview_load(&mut self) {
        self.preview_epoch.fetch_add(1, Ordering::AcqRel);
        if let Some(cancel) = self.preview_cancellation.take() {
            cancel.cancel();
        }
    }

    pub(crate) fn drain_preview_results(&mut self, app: &mut crate::tui::TuiApp) {
        while let Ok(result) = self.preview_rx.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            match result.result {
                Ok(bytes) if result.kind == PreviewFetchKind::Text => {
                    let key = result.request.key();
                    let status = self
                        .preview_manager
                        .finish_text_request(result.request, &bytes);
                    if let Some(status) = status {
                        if let Some(text) = self.preview_manager.get(&key) {
                            let content = text.content.clone();
                            if self.selected_preview_key(app) == Some(key) {
                                app.finish_preview_text(
                                    result.title.as_str(),
                                    content.as_str(),
                                    status,
                                );
                            }
                        }
                    }
                }
                Ok(bytes) => {
                    let key = result.request.key();
                    if self.preview_manager.is_current_request(result.request) {
                        let metadata = crate::preview::parse_image_metadata(&bytes);
                        let status = crate::preview::PreviewStatus {
                            kind: crate::preview::PreviewKind::Image,
                            fetched_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                            memory_limit_bytes: PREVIEW_MEMORY_LIMIT_BYTES,
                        };
                        if self.selected_preview_key(app) == Some(key) {
                            app.finish_preview_image_metadata(
                                result.title.as_str(),
                                image_preview_lines(metadata),
                                crate::preview::PreviewStatus {
                                    kind: crate::preview::PreviewKind::Image,
                                    fetched_bytes: status.fetched_bytes,
                                    memory_limit_bytes: PREVIEW_MEMORY_LIMIT_BYTES,
                                },
                            );
                        }
                    }
                }
                Err(error) => {
                    if self.selected_preview_key(app) == Some(result.request.key()) {
                        app.finish_preview_error(
                            result.title.as_str(),
                            format!("preview_error: {error}"),
                        );
                    }
                }
            }
        }
        app.set_background_tasks(self.inflight);
    }

    pub(crate) fn start_download_task(&mut self, app: &mut crate::tui::TuiApp) {
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
        let id = self
            .task_manager
            .enqueue_download(target.to_string_lossy().as_ref(), size);
        if self.task_manager.start(id).is_err() {
            app.set_notice("download_error: failed to start task");
            return;
        }

        let cancellation = self.root.child_token();
        self.task_cancellations.insert(id, cancellation.clone());
        let tx = self.task_tx.clone();
        let remote_path = entry.path;
        let account_uid = self.account_uid;
        self.tasks.spawn(async move {
            let worker = DownloadWorker {
                id,
                client,
                remote_path,
                target,
                size,
                account_uid,
                account_bduss,
                cancellation,
                tx,
                mode: TransferWorkerMode::Download,
            };
            run_download_worker(worker).await;
        });
        app.set_notice(format!(
            "download_started: #{} {}",
            id.value(),
            target_label
        ));
    }

    pub(crate) fn cancel_latest_task(&mut self, app: &mut crate::tui::TuiApp) {
        let Some(id) = self
            .task_manager
            .snapshots()
            .into_iter()
            .rev()
            .find(|task| {
                matches!(
                    task.state,
                    crate::app::TaskState::Queued | crate::app::TaskState::Running
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
            cancellation.cancel();
        }
        app.set_notice(format!("task_cancel_requested: #{}", id.value()));
    }

    pub(crate) fn drain_transfer_task_events(&mut self, app: &mut crate::tui::TuiApp) {
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
                TransferTaskEvent::Completed { id, mode } => {
                    let _ = self.task_manager.complete(id);
                    self.task_cancellations.remove(&id);
                    if mode == TransferWorkerMode::LocalOpen {
                        app.set_notice(format!("open_complete: #{}", id.value()));
                    } else {
                        app.set_notice(format!("download_complete: #{}", id.value()));
                        let preferred = app.selected_entry().map(|entry| entry.path.clone());
                        self.start_directory_load(
                            app,
                            app.path().to_string(),
                            preferred.as_deref(),
                        );
                    }
                }
                TransferTaskEvent::Failed { id, mode, message } => {
                    let _ = self.task_manager.fail(id);
                    self.task_cancellations.remove(&id);
                    if mode == TransferWorkerMode::LocalOpen {
                        app.set_notice(format!("open_failed: #{} {}", id.value(), message));
                    } else {
                        app.set_notice(format!("download_failed: #{} {}", id.value(), message));
                    }
                }
                TransferTaskEvent::Cancelled { id, mode, .. } => {
                    let _ = self.task_manager.cancelled(id);
                    self.task_cancellations.remove(&id);
                    if mode == TransferWorkerMode::LocalOpen {
                        app.set_notice(format!("open_cancelled: #{}", id.value()));
                    } else {
                        app.set_notice(format!("download_cancelled: #{}", id.value()));
                    }
                }
                TransferTaskEvent::RemoteCompleted {
                    id,
                    label,
                    affected_dirs,
                } => {
                    let _ = self.task_manager.complete(id);
                    self.task_cancellations.remove(&id);
                    app.set_notice(format!("operation_complete: #{} {}", id.value(), label));
                    self.refresh_related_dirs(app, affected_dirs);
                }
                TransferTaskEvent::RemoteFailed { id, message } => {
                    let _ = self.task_manager.fail(id);
                    self.task_cancellations.remove(&id);
                    app.set_notice(format!("operation_failed: #{} {}", id.value(), message));
                }
                TransferTaskEvent::SearchBatch {
                    id,
                    path,
                    entries,
                    scanned_dirs,
                    scanned_entries,
                } => {
                    let expected_path = app.search_origin_path().map(str::to_string);
                    if expected_path.as_deref() == Some(path.as_str()) {
                        app.append_search_results(entries, scanned_dirs, scanned_entries);
                        app.set_notice(format!(
                            "search_progress: #{} {} dirs, {} entries",
                            id.value(),
                            scanned_dirs,
                            scanned_entries
                        ));
                        self.update_preview_for_selection(app);
                    }
                }
                TransferTaskEvent::SearchCompleted {
                    id,
                    path,
                    keyword,
                    result,
                } => {
                    self.task_cancellations.remove(&id);
                    match result {
                        Ok(summary) => {
                            let _ = self.task_manager.complete(id);
                            app.finish_search_success(
                                summary.scanned_dirs,
                                summary.scanned_entries,
                            );
                            app.set_notice(format!(
                                "search_complete: #{} {} matches for {} in {}",
                                id.value(),
                                summary.matches,
                                keyword,
                                path
                            ));
                            self.update_preview_for_selection(app);
                        }
                        Err(message) => {
                            let _ = self.task_manager.fail(id);
                            app.finish_search_error(format!("#{} {}", id.value(), message));
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn refresh_related_dirs(&mut self, app: &mut crate::tui::TuiApp, dirs: Vec<String>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let mut refreshed_current = false;
        for dir in dirs {
            if dir == app.path() {
                if !refreshed_current {
                    refreshed_current = true;
                    let preferred = app.selected_entry().map(|entry| entry.path.clone());
                    self.start_directory_load(app, dir, preferred.as_deref());
                }
            } else {
                let generation = self.cache.begin_refresh(dir.as_str());
                let cancel = self.root.child_token();
                self.spawn_directory_load(client.clone(), dir, generation, cancel);
            }
        }
    }

    pub(crate) fn start_preview_action(&mut self, app: &mut crate::tui::TuiApp) {
        let Some(entry) = app.selected_entry().cloned() else {
            app.set_notice("preview: no selected entry");
            return;
        };
        if is_video_entry(&entry) {
            self.start_video_preview(app);
        } else {
            self.start_local_open_preview(app, entry);
        }
    }

    pub(crate) fn start_local_open_preview(
        &mut self,
        app: &mut crate::tui::TuiApp,
        entry: crate::pcs::FileEntry,
    ) {
        if entry.is_dir {
            app.set_notice("open: directories cannot be opened as local files");
            return;
        }
        let size = nonnegative_size(entry.size);
        if !is_local_open_size_allowed(size) {
            app.set_notice(format!(
                "open_skipped: {} exceeds 100MiB limit",
                entry.server_filename
            ));
            return;
        }
        let Some(client) = self.client.clone() else {
            app.set_notice("open_error: client unavailable");
            return;
        };
        let Some(account_bduss) = self.account_bduss.clone() else {
            app.set_notice("open_auth_error: missing BDUSS");
            return;
        };
        if self.account_uid == 0 {
            app.set_notice("open_auth_error: missing uid");
            return;
        }

        let target = local_open_target_path(
            env::temp_dir().join(LOCAL_OPEN_TEMP_DIR_NAME).as_path(),
            self.account_uid,
            &entry,
            size,
        );
        let target_label = file_name_label(target.to_string_lossy().as_ref());
        let id = self
            .task_manager
            .enqueue_local_open(target.to_string_lossy().as_ref(), size);
        if self.task_manager.start(id).is_err() {
            app.set_notice("open_error: failed to start task");
            return;
        }

        let cancellation = self.root.child_token();
        self.task_cancellations.insert(id, cancellation.clone());
        let tx = self.task_tx.clone();
        let remote_path = entry.path;
        let account_uid = self.account_uid;
        self.tasks.spawn(async move {
            let worker = DownloadWorker {
                id,
                client,
                remote_path,
                target,
                size,
                account_uid,
                account_bduss,
                cancellation,
                tx,
                mode: TransferWorkerMode::LocalOpen,
            };
            run_download_worker(worker).await;
        });
        app.set_notice(format!("open_started: #{} {}", id.value(), target_label));
    }

    pub(crate) fn start_video_preview(&mut self, app: &mut crate::tui::TuiApp) {
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
                crate::preview::PreviewKind::Video,
                "video preview disabled by sidecar kill switch",
                crate::preview::PreviewStatus {
                    kind: crate::preview::PreviewKind::Video,
                    fetched_bytes: 0,
                    memory_limit_bytes: self.video_config.memory_limit_bytes(),
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
        let budget = match crate::preview::VideoCacheBudget::new(
            self.video_config.chunk_size_bytes,
            self.video_config.max_chunks,
        ) {
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
        let upstream = Arc::new(PcsVideoUpstream::new(
            client,
            path,
            account_uid,
            account_bduss,
        ));
        let cancel = self.root.child_token();
        let server = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                crate::preview::VideoLoopbackServer::spawn_with_prefetch(
                    nonnegative_size(entry.size),
                    token,
                    budget,
                    default_video_prefetch_chunks(self.video_config.max_chunks),
                    upstream,
                    cancel,
                ),
            )
        });
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
        let mpv_args = match server.mpv_args_with_profile(self.video_config.player_profile) {
            Ok(args) => args,
            Err(err) => {
                app.finish_preview_error(
                    entry.server_filename.as_str(),
                    format!("video_player_error: {err:?}"),
                );
                return;
            }
        };
        if !program_available(MPV_PROGRAM) {
            app.finish_preview_error(
                entry.server_filename.as_str(),
                format!("video_player_missing: {MPV_PROGRAM} not found on PATH"),
            );
            return;
        }
        let mut command = tokio::process::Command::new(MPV_PROGRAM);
        command
            .args(&mpv_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = command.spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => {
                app.finish_preview_error(
                    entry.server_filename.as_str(),
                    format!("video_player_spawn_error: {err}"),
                );
                return;
            }
        };
        let player_log = Arc::new(Mutex::new(crate::preview::PlayerLogRing::new(6)));
        if let Some(stderr) = child.stderr.take() {
            let player_log = Arc::clone(&player_log);
            self.tasks.spawn(async move {
                use tokio::io::AsyncBufReadExt;

                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(mut guard) = player_log.lock() else {
                        break;
                    };
                    guard.push(line.as_str());
                }
            });
        }
        self.active_video = Some(ActiveVideoPreview {
            key,
            title,
            server,
            player: child,
            player_log,
        });
        self.refresh_active_video_preview(app);
    }

    pub(crate) fn refresh_active_video_preview(&mut self, app: &mut crate::tui::TuiApp) {
        if self.active_video.is_none() {
            return;
        }
        self.stop_active_video_if_selection_changed(app);
        let Some(active) = self.active_video.as_mut() else {
            return;
        };
        let player_status = match active.player.try_wait() {
            Ok(Some(status)) => Some(format!("player_exit={status}")),
            Ok(None) => None,
            Err(err) => Some(format!("player_poll_error={err}")),
        };
        let playback_state = if player_status.is_some() {
            "failed"
        } else {
            "playing"
        };
        let stats = active.server.stats();
        let memory_limit = self.video_config.memory_limit_bytes();
        let player_line = player_status.unwrap_or_else(|| {
            active.player.id().map_or_else(
                || "player=mpv pid=unknown".to_string(),
                |pid| format!("player=mpv pid={pid}"),
            )
        });
        let video_error = stats
            .last_error
            .as_ref()
            .map_or_else(|| "none".to_string(), |error| format!("{error:?}"));
        let lines = vec![
            "video=loopback seek=true".to_string(),
            format!("playback={playback_state}"),
            format!("mpv_profile={}", self.video_config.player_profile.as_str()),
            active_player_log_line(&active.player_log),
            format!("listen=127.0.0.1:{}", active.server.port()),
            player_line,
            format!(
                "range={} window={}",
                video_range_label(stats.last_range_start, stats.last_range_end_inclusive),
                video_range_label(stats.window_start, stats.window_end_inclusive)
            ),
            format!(
                "buffer={}/{} hits={} upstream={} prefetch={}",
                stats.used_bytes,
                memory_limit,
                stats.cache_hits,
                stats.upstream_fetches,
                stats.prefetch_fetches
            ),
            format!("error={video_error}"),
        ];
        app.finish_preview_video(
            active.title.as_str(),
            lines,
            crate::preview::PreviewStatus {
                kind: crate::preview::PreviewKind::Video,
                fetched_bytes: stats.used_bytes,
                memory_limit_bytes: memory_limit,
            },
        );
    }

    pub(crate) fn stop_active_video_if_selection_changed(&mut self, app: &crate::tui::TuiApp) {
        let active_key = self.active_video.as_ref().map(|active| active.key);
        if active_key.is_some() && self.selected_preview_key(app) != active_key {
            self.stop_active_video();
        }
    }

    pub(crate) fn stop_active_video(&mut self) {
        self.video_epoch.fetch_add(1, Ordering::AcqRel);
        self.active_video = None;
    }

    pub(crate) async fn shutdown_active_video(&mut self) {
        self.video_epoch.fetch_add(1, Ordering::AcqRel);
        let Some(active) = self.active_video.take() else {
            return;
        };
        let ActiveVideoPreview {
            mut player, server, ..
        } = active;
        let _ = player.start_kill();
        let _ = server.shutdown().await;
    }

    pub(crate) fn selected_preview_key(
        &self,
        app: &crate::tui::TuiApp,
    ) -> Option<crate::preview::PreviewKey> {
        app.selected_entry()
            .map(|entry| preview_key_for_entry(self.account_uid, entry))
    }

    pub(crate) fn refresh_current_cache_age(&self, app: &mut crate::tui::TuiApp) {
        app.set_cache_age_seconds(self.cache_age_seconds(app.path()));
        app.set_cache_source(self.cache_source_for_path(app.path()));
    }

    pub(crate) fn refresh_task_panel(&self, app: &mut crate::tui::TuiApp) {
        let snapshots: Vec<_> = self
            .task_manager
            .snapshots()
            .into_iter()
            .rev()
            .take(3)
            .collect();
        let items = snapshots.iter().map(task_panel_item).collect();
        app.set_task_items(items);
        app.set_background_tasks(self.inflight + self.task_manager.active_count());
    }

    pub(crate) fn cache_age_seconds(&self, path: &str) -> Option<u64> {
        self.loaded_at
            .get(path)
            .and_then(|loaded_at| SystemTime::now().duration_since(*loaded_at).ok())
            .map(|duration| duration.as_secs())
    }

    pub(crate) fn cache_source_for_path(&self, path: &str) -> crate::tui::CacheSource {
        self.cache_sources
            .get(path)
            .copied()
            .unwrap_or(crate::tui::CacheSource::None)
    }

    pub(crate) fn hydrate_directory_cache_from_disk(&mut self, path: &str) {
        if self.cache.snapshot(path).status != crate::app::DirectoryStatus::Empty {
            return;
        }
        let Ok(Some(snapshot)) = self.cache_store.read_snapshot(self.account_uid, path) else {
            return;
        };
        self.loaded_at.insert(
            path.to_string(),
            UNIX_EPOCH + Duration::from_secs(snapshot.saved_at_unix_seconds),
        );
        self.cache_sources
            .insert(path.to_string(), crate::tui::CacheSource::Disk);
        self.cache
            .replace_with_fresh_entries(path, snapshot.entries);
    }

    pub(crate) fn persist_directory_cache(
        &mut self,
        path: &str,
        entries: Vec<crate::pcs::FileEntry>,
    ) {
        let store = self.cache_store.clone();
        let path = path.to_string();
        let account_uid = self.account_uid;
        let saved_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        self.tasks.spawn_blocking(move || {
            let _ = store.write_snapshot(account_uid, path.as_str(), entries, saved_at);
        });
    }
}
