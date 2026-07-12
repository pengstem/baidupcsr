use super::*;

pub(crate) fn build_pcs_client(
    config: &crate::config::PcsConfig,
    user: &crate::config::BaiduUser,
) -> Result<crate::pcs::PcsClient, String> {
    let auth = if user.cookies.is_empty() {
        crate::pcs::PcsAuth::from_bduss_stoken(
            user.bduss.as_str(),
            Some(user.stoken.as_str()),
            Some(user.baiduid.as_str()),
        )
    } else {
        Some(crate::pcs::PcsAuth::from_cookie_header(
            user.cookies.as_str(),
        ))
    }
    .ok_or_else(|| "missing BDUSS/cookies in active user".to_string())?;

    let pcs_addr = if config.pcs_addr.is_empty() {
        "pcs.baidu.com"
    } else {
        config.pcs_addr.as_str()
    };
    let user_agent = pcs_user_agent_for_config(config);
    let pan_user_agent = pan_user_agent_for_config(config);

    crate::pcs::PcsClient::new(
        crate::pcs::PcsClientOptions {
            app_id: config.appid,
            pcs_addr: pcs_addr.to_string(),
            enable_https: config.enable_https,
        },
        auth,
        crate::pcs::PcsTransportOptions::new_with_pan(
            user_agent,
            pan_user_agent,
            proxy_from_config_or_env(config.proxy.as_str(), env::var_os(ENV_PROXY)),
        ),
    )
    .map_err(|error| error.to_string())
}

pub(crate) struct DownloadWorker {
    pub(crate) id: crate::app::TaskId,
    pub(crate) client: crate::pcs::PcsClient,
    pub(crate) remote_path: String,
    pub(crate) target: PathBuf,
    pub(crate) size: u64,
    pub(crate) account_uid: u64,
    pub(crate) account_bduss: String,
    pub(crate) cancellation: tokio_util::sync::CancellationToken,
    pub(crate) tx: mpsc::Sender<TransferTaskEvent>,
    pub(crate) mode: TransferWorkerMode,
}

pub(crate) struct PcsVideoUpstream {
    pub(crate) client: crate::pcs::PcsClient,
    pub(crate) path: String,
    pub(crate) account_uid: u64,
    pub(crate) account_bduss: String,
    pub(crate) urls: tokio::sync::Mutex<Option<Vec<String>>>,
}

impl PcsVideoUpstream {
    pub(crate) fn new(
        client: crate::pcs::PcsClient,
        path: String,
        account_uid: u64,
        account_bduss: String,
    ) -> Self {
        Self {
            client,
            path,
            account_uid,
            account_bduss,
            urls: tokio::sync::Mutex::new(None),
        }
    }

    pub(crate) async fn located_urls(
        &self,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<Vec<String>, crate::pcs::PcsError> {
        let mut cached = self.urls.lock().await;
        if let Some(urls) = cached.as_ref() {
            return Ok(urls.clone());
        }
        let urls = self
            .client
            .locate_download_urls(&self.path, self.account_uid, &self.account_bduss, cancel)
            .await?;
        *cached = Some(urls.clone());
        Ok(urls)
    }

    pub(crate) async fn refresh_urls_if_stale(
        &self,
        stale: &[String],
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<Vec<String>, crate::pcs::PcsError> {
        let mut cached = self.urls.lock().await;
        if cached.as_deref().is_some_and(|urls| urls != stale) {
            return Ok(cached.clone().unwrap_or_default());
        }
        let urls = self
            .client
            .locate_download_urls(&self.path, self.account_uid, &self.account_bduss, cancel)
            .await?;
        *cached = Some(urls.clone());
        Ok(urls)
    }

    pub(crate) async fn download_from_candidates(
        &self,
        urls: &[String],
        range: crate::pcs::ByteRange,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(usize, Vec<u8>), crate::pcs::PcsError> {
        let mut last_error = None;
        for (index, url) in urls.iter().enumerate() {
            match self.client.download_url_range(url, range, cancel).await {
                Ok(bytes) => return Ok((index, bytes)),
                Err(error @ crate::pcs::PcsError::Cancelled { .. }) => return Err(error),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| crate::pcs::PcsError::Parse {
            operation: "download".to_string(),
            message: "no download URL candidates".to_string(),
        }))
    }

    pub(crate) async fn promote_successful_url(&self, urls: &[String], index: usize) {
        let Some(url) = urls.get(index) else {
            return;
        };
        let mut cached = self.urls.lock().await;
        let Some(cached_urls) = cached.as_mut() else {
            return;
        };
        if cached_urls.as_slice() != urls {
            return;
        }
        if index != 0 {
            let url = url.clone();
            cached_urls.remove(index);
            cached_urls.insert(0, url);
        }
    }

    pub(crate) async fn fetch_pcs_range(
        &self,
        range: crate::pcs::ByteRange,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<Vec<u8>, crate::pcs::PcsError> {
        let urls = self.located_urls(cancel).await?;
        match self.download_from_candidates(&urls, range, cancel).await {
            Ok((index, bytes)) => {
                self.promote_successful_url(&urls, index).await;
                Ok(bytes)
            }
            Err(error @ crate::pcs::PcsError::Cancelled { .. }) => Err(error),
            Err(_) => {
                let refreshed = self.refresh_urls_if_stale(&urls, cancel).await?;
                match self
                    .download_from_candidates(&refreshed, range, cancel)
                    .await
                {
                    Ok((index, bytes)) => {
                        self.promote_successful_url(&refreshed, index).await;
                        Ok(bytes)
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }
}

impl crate::app::RangeFetcher for PcsVideoUpstream {
    fn fetch_range<'a>(
        &'a self,
        _path: &'a str,
        range: crate::pcs::ByteRange,
        cancel: &'a tokio_util::sync::CancellationToken,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<u8>, crate::pcs::PcsError>> + Send + 'a>,
    > {
        Box::pin(self.fetch_pcs_range(range, cancel))
    }
}

impl crate::preview::VideoUpstream for PcsVideoUpstream {
    fn fetch<'a>(
        &'a self,
        range: crate::preview::VideoRange,
        cancel: &'a tokio_util::sync::CancellationToken,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<u8>, crate::preview::VideoError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let range = crate::pcs::ByteRange::new(range.start, range.end_inclusive)
                .map_err(pcs_to_video_error)?;
            self.fetch_pcs_range(range, cancel)
                .await
                .map_err(pcs_to_video_error)
        })
    }
}

pub(crate) fn pcs_to_video_error(error: crate::pcs::PcsError) -> crate::preview::VideoError {
    match error {
        crate::pcs::PcsError::Cancelled { .. } => crate::preview::VideoError::Cancelled,
        error => crate::preview::VideoError::Upstream(error.to_string()),
    }
}

pub(crate) async fn run_download_worker(worker: DownloadWorker) {
    let id = worker.id;
    let mode = worker.mode;
    let mut spec = crate::app::DownloadSpec::new(
        worker.remote_path.clone(),
        worker.target.clone(),
        worker.size,
    );
    spec.existing_policy = match mode {
        TransferWorkerMode::Download => crate::app::ExistingTargetPolicy::FailIfExists,
        TransferWorkerMode::LocalOpen => crate::app::ExistingTargetPolicy::ReuseIfComplete,
    };
    let fetcher = PcsVideoUpstream::new(
        worker.client.clone(),
        worker.remote_path.clone(),
        worker.account_uid,
        worker.account_bduss.clone(),
    );

    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::channel::<crate::app::TransferProgress>(256);
    let event_tx = worker.tx.clone();
    tokio::spawn(async move {
        let started_at = Instant::now();
        while let Some(progress) = progress_rx.recv().await {
            let _ = event_tx
                .send(TransferTaskEvent::Progress {
                    id,
                    bytes_done: progress.bytes_done,
                    elapsed_seconds: started_at.elapsed().as_secs().max(1),
                })
                .await;
        }
    });

    let result =
        crate::app::download_to_path(&fetcher, &spec, progress_tx, worker.cancellation.clone())
            .await;
    let event = match result {
        Ok(crate::app::DownloadOutcome::Completed { .. }) => {
            if mode == TransferWorkerMode::LocalOpen {
                match spawn_local_file_opener(&worker.target) {
                    Ok(()) => TransferTaskEvent::Completed { id, mode },
                    Err(message) => TransferTaskEvent::Failed { id, mode, message },
                }
            } else {
                TransferTaskEvent::Completed { id, mode }
            }
        }
        Ok(crate::app::DownloadOutcome::Cancelled { .. }) => {
            TransferTaskEvent::Cancelled { id, mode }
        }
        Err(error) => TransferTaskEvent::Failed {
            id,
            mode,
            message: error.to_string(),
        },
    };
    let _ = worker.tx.send(event).await;
}

pub(crate) fn is_video_entry(entry: &crate::pcs::FileEntry) -> bool {
    !entry.is_dir
        && crate::preview::preview_kind_from_name(entry.server_filename.as_str())
            == crate::preview::PreviewKind::Video
}

pub(crate) fn default_video_prefetch_chunks(max_chunks: usize) -> usize {
    max_chunks.saturating_sub(1).min(VIDEO_PREFETCH_CHUNK_LIMIT)
}

pub(crate) fn active_player_log_line(log: &Arc<Mutex<crate::preview::PlayerLogRing>>) -> String {
    match log
        .lock()
        .ok()
        .and_then(|guard| guard.last_line().map(str::to_string))
    {
        Some(line) if !line.is_empty() => format!("player_log={line}"),
        _ => "player_log=none".to_string(),
    }
}

pub(crate) fn video_range_label(start: Option<u64>, end_inclusive: Option<u64>) -> String {
    match (start, end_inclusive) {
        (Some(start), Some(end_inclusive)) => format!("{start}-{end_inclusive}"),
        _ => "empty".to_string(),
    }
}

pub(crate) fn image_preview_lines(metadata: crate::preview::ImagePreviewMetadata) -> Vec<String> {
    let format = match metadata.format {
        crate::preview::ImagePreviewFormat::Png => "png",
        crate::preview::ImagePreviewFormat::Jpeg => "jpeg",
        crate::preview::ImagePreviewFormat::Unknown => "unknown",
    };
    let dimensions = match (metadata.width, metadata.height) {
        (Some(width), Some(height)) => format!("{width}x{height}"),
        _ => "unknown".to_string(),
    };
    vec![
        format!("image={format}"),
        format!("dimensions={dimensions}"),
        "terminal=metadata".to_string(),
    ]
}

pub(crate) fn program_available(program: &str) -> bool {
    if program.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(program).is_file();
    }
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(program);
            candidate.is_file()
        })
    })
}

pub(crate) fn download_target_path(download_dir: &Path, file_name: &str) -> PathBuf {
    download_dir.join(sanitize_download_file_name(file_name))
}

pub(crate) fn local_open_target_path(
    root: &Path,
    account_uid: u64,
    entry: &crate::pcs::FileEntry,
    size: u64,
) -> PathBuf {
    root.join(account_uid.to_string()).join(format!(
        "{}-{}-{}-{}",
        entry.fs_id,
        entry.mtime,
        size,
        sanitize_download_file_name(entry.server_filename.as_str())
    ))
}

pub(crate) const fn is_local_open_size_allowed(size: u64) -> bool {
    size < LOCAL_OPEN_MAX_BYTES
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalOpenCommand {
    pub(crate) program: &'static str,
    pub(crate) args: Vec<OsString>,
}

pub(crate) fn spawn_local_file_opener(path: &Path) -> Result<(), String> {
    let mut last_error = "opener_unavailable".to_string();
    for command in local_open_command_candidates(path) {
        match Command::new(command.program)
            .args(command.args.iter())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(err) => last_error = io_error_label(&err),
        }
    }
    Err(format!("open_failed: {last_error}"))
}

#[cfg(target_os = "linux")]
pub(crate) fn local_open_command_candidates(path: &Path) -> Vec<LocalOpenCommand> {
    vec![
        LocalOpenCommand {
            program: "xdg-open",
            args: vec![path.as_os_str().to_os_string()],
        },
        LocalOpenCommand {
            program: "gio",
            args: vec![OsString::from("open"), path.as_os_str().to_os_string()],
        },
    ]
}

#[cfg(target_os = "macos")]
pub(crate) fn local_open_command_candidates(path: &Path) -> Vec<LocalOpenCommand> {
    vec![LocalOpenCommand {
        program: "open",
        args: vec![path.as_os_str().to_os_string()],
    }]
}

#[cfg(target_os = "windows")]
pub(crate) fn local_open_command_candidates(path: &Path) -> Vec<LocalOpenCommand> {
    vec![LocalOpenCommand {
        program: "cmd",
        args: vec![
            OsString::from("/C"),
            OsString::from("start"),
            OsString::new(),
            path.as_os_str().to_os_string(),
        ],
    }]
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn local_open_command_candidates(_path: &Path) -> Vec<LocalOpenCommand> {
    Vec::new()
}

pub(crate) async fn run_remote_search_task(
    client: crate::pcs::PcsClient,
    tx: mpsc::Sender<TransferTaskEvent>,
    id: crate::app::TaskId,
    path: &str,
    keyword: &str,
    mode: RemoteSearchMode,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<RemoteSearchSummary, crate::pcs::PcsError> {
    match mode {
        RemoteSearchMode::PcsApi => {
            let entries = client.search(path, keyword, true, cancel).await?;
            let summary = RemoteSearchSummary {
                scanned_dirs: 1,
                scanned_entries: entries.len(),
                matches: entries.len(),
            };
            send_search_batch(&tx, id, path, entries, summary, cancel).await?;
            Ok(summary)
        }
        RemoteSearchMode::RecursiveList | RemoteSearchMode::CurrentList => {
            let recursive = mode == RemoteSearchMode::RecursiveList;
            let needle = keyword.trim().to_lowercase();
            let mut queue = VecDeque::from([path.to_string()]);
            let mut summary = RemoteSearchSummary::default();
            while let Some(directory) = queue.pop_front() {
                summary.scanned_dirs = summary.scanned_dirs.saturating_add(1);
                let entries = client.list(&directory, cancel).await?;
                let mut batch = Vec::new();
                for entry in entries {
                    summary.scanned_entries = summary.scanned_entries.saturating_add(1);
                    if recursive && entry.is_dir {
                        queue.push_back(entry.path.clone());
                    }
                    if entry.server_filename.to_lowercase().contains(&needle) {
                        summary.matches = summary.matches.saturating_add(1);
                        batch.push(entry);
                        if batch.len() >= SEARCH_BATCH_SIZE {
                            send_search_batch(
                                &tx,
                                id,
                                path,
                                std::mem::take(&mut batch),
                                summary,
                                cancel,
                            )
                            .await?;
                        }
                        if summary.matches >= SEARCH_MAX_RESULTS {
                            send_search_batch(&tx, id, path, batch, summary, cancel).await?;
                            return Ok(summary);
                        }
                    }
                }
                send_search_batch(&tx, id, path, batch, summary, cancel).await?;
                if !recursive {
                    break;
                }
            }
            Ok(summary)
        }
    }
}

pub(crate) async fn send_search_batch(
    tx: &mpsc::Sender<TransferTaskEvent>,
    id: crate::app::TaskId,
    path: &str,
    entries: Vec<crate::pcs::FileEntry>,
    summary: RemoteSearchSummary,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(), crate::pcs::PcsError> {
    if entries.is_empty() {
        return Ok(());
    }
    tokio::select! {
        _ = cancel.cancelled() => Err(crate::pcs::PcsError::Cancelled {
            operation: "search".to_string(),
        }),
        result = tx.send(TransferTaskEvent::SearchBatch {
            id,
            path: path.to_string(),
            entries,
            scanned_dirs: summary.scanned_dirs,
            scanned_entries: summary.scanned_entries,
        }) => result.map_err(|_| crate::pcs::PcsError::Cancelled {
            operation: "search".to_string(),
        }),
    }
}
