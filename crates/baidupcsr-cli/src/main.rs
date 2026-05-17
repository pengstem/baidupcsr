use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::Instant;

const PREVIEW_MEMORY_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
const TEXT_PREVIEW_FETCH_BYTES: u64 = 64 * 1024;

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
    cache: baidupcsr_core::DirectoryCache,
    loaded_at: HashMap<String, Instant>,
    selection_memory: HashMap<String, String>,
    preview_manager: baidupcsr_preview::PreviewManager,
    preview_epoch: Arc<AtomicU64>,
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

impl BrowserRuntime {
    fn from_config() -> Self {
        let (tx, rx) = mpsc::channel();
        let (preview_tx, preview_rx) = mpsc::channel();
        let discovery = baidupcsr_config::discovery_input_from_env(None);
        let config_path = baidupcsr_config::discover_config_file(&discovery);
        let config = match baidupcsr_config::load_config(&config_path) {
            Ok(config) => config,
            Err(err) => {
                return Self {
                    client: None,
                    account_uid: 0,
                    cache: baidupcsr_core::DirectoryCache::new(),
                    loaded_at: HashMap::new(),
                    selection_memory: HashMap::new(),
                    preview_manager: baidupcsr_preview::PreviewManager::new(
                        PREVIEW_MEMORY_LIMIT_BYTES,
                    ),
                    preview_epoch: Arc::new(AtomicU64::new(0)),
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
                cache: baidupcsr_core::DirectoryCache::new(),
                loaded_at: HashMap::new(),
                selection_memory: HashMap::new(),
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
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

        match client {
            Ok(client) => Self {
                client: Some(client),
                account_uid: user.uid,
                cache: baidupcsr_core::DirectoryCache::new(),
                loaded_at: HashMap::new(),
                selection_memory: HashMap::new(),
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
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
                cache: baidupcsr_core::DirectoryCache::new(),
                loaded_at: HashMap::new(),
                selection_memory: HashMap::new(),
                preview_manager: baidupcsr_preview::PreviewManager::new(PREVIEW_MEMORY_LIMIT_BYTES),
                preview_epoch: Arc::new(AtomicU64::new(0)),
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
    let user_agent_values = [
        config.pcs_ua.as_str(),
        config.user_agent.as_str(),
        config.pan_ua.as_str(),
    ];
    let user_agent = first_non_empty(&user_agent_values).unwrap_or("netdisk");

    let api = baidupcsr_pcs::PcsApi::new(baidupcsr_pcs::PcsClientOptions {
        app_id: config.appid,
        pcs_addr: pcs_addr.to_string(),
        enable_https: config.enable_https,
    });
    Ok(baidupcsr_pcs::BlockingPcsClient::new(
        api,
        auth,
        baidupcsr_pcs::BlockingPcsClientOptions::new(
            user_agent,
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

fn first_non_empty<'a>(values: &'a [&str]) -> Option<&'a str> {
    values.iter().copied().find(|value| !value.is_empty())
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
    use baidupcsr_pcs::FileEntry;

    use super::{preview_key_for_entry, tui_preview_kind};

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
}
