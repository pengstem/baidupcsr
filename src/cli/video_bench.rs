use super::*;
use futures_util::StreamExt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VideoBenchResolvedOptions {
    pub(crate) chunk_size_bytes: u64,
    pub(crate) max_chunks: usize,
    pub(crate) prefetch_chunks: usize,
    pub(crate) memory_limit_bytes: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct VideoBenchMovie {
    pub(crate) entry: crate::pcs::FileEntry,
    pub(crate) size_bytes: u64,
    pub(crate) path_hash: String,
    pub(crate) extension: String,
}

pub(crate) struct VideoBenchOutput {
    pub(crate) writer: Box<dyn Write>,
}

impl VideoBenchOutput {
    pub(crate) fn new(path: Option<&Path>) -> Result<Self, String> {
        let writer: Box<dyn Write> = match path {
            Some(path) => {
                if let Some(parent) = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    fs::create_dir_all(parent).map_err(|err| io_error_label(&err))?;
                }
                let file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(path)
                    .map_err(|err| io_error_label(&err))?;
                Box::new(BufWriter::new(file))
            }
            None => Box::new(std::io::stdout()),
        };
        Ok(Self { writer })
    }

    pub(crate) fn emit<T: serde::Serialize + ?Sized>(&mut self, event: &T) -> Result<(), String> {
        serde_json::to_writer(&mut self.writer, event).map_err(|err| error_chain_label(&err))?;
        self.writer
            .write_all(b"\n")
            .map_err(|err| io_error_label(&err))
    }

    pub(crate) fn flush(&mut self) -> Result<(), String> {
        self.writer.flush().map_err(|err| io_error_label(&err))
    }
}

pub(crate) async fn run_video_bench(options: &VideoBenchCliOptions) -> Result<(), String> {
    let config_path = crate::config::discover_config_file(None);
    let config = crate::config::load_config(&config_path)
        .map_err(|err| format!("config_load_failed: {err}"))?;
    let user = config
        .active_user()
        .ok_or_else(|| "active_user_missing".to_string())?;
    let account_bduss = bduss_from_user(user).ok_or_else(|| "bduss_missing".to_string())?;
    let client = build_pcs_client(&config, user)?;
    let ui_config_path = crate::config::discover_ui_config_file();
    let ui_config = crate::config::load_ui_config_or_default(&ui_config_path)
        .unwrap_or_else(|_| crate::config::UiConfig::default());
    let resolved = resolve_video_bench_options(options, &ui_config.video_preview)?;
    let movie = resolve_video_bench_movie(&client, options.remote_path.as_str()).await?;
    if !is_video_entry(&movie.entry) {
        return Err("target_is_not_supported_video".to_string());
    }

    let mut output = VideoBenchOutput::new(options.output_path.as_deref())?;
    output.emit(&format_video_bench_start(
        options, &resolved, &movie, user.uid,
    ))?;
    if options.bench_case.includes_source() {
        run_video_source_bench(
            &mut output,
            &client,
            &movie,
            user.uid,
            account_bduss.as_str(),
            options,
        )
        .await?;
    }
    if options.bench_case.includes_gateway() {
        run_video_gateway_bench(
            &mut output,
            client,
            &movie,
            user.uid,
            account_bduss,
            &resolved,
            options.duration_seconds,
        )
        .await?;
    }
    output.emit(&format_video_bench_end(options))?;
    output.flush()
}

pub(crate) fn resolve_video_bench_options(
    options: &VideoBenchCliOptions,
    video_config: &crate::config::VideoPreviewConfig,
) -> Result<VideoBenchResolvedOptions, String> {
    let chunk_size_bytes = options
        .chunk_size_bytes
        .unwrap_or(video_config.chunk_size_bytes);
    let max_chunks = options.max_chunks.unwrap_or(video_config.max_chunks);
    let prefetch_chunks = options
        .prefetch_chunks
        .unwrap_or_else(|| default_video_prefetch_chunks(max_chunks));
    crate::preview::VideoCacheBudget::new(chunk_size_bytes, max_chunks)
        .map_err(|err| format!("video_budget_error: {err:?}"))?;
    let memory_limit_bytes =
        chunk_size_bytes.saturating_mul(u64::try_from(max_chunks).unwrap_or(u64::MAX));
    Ok(VideoBenchResolvedOptions {
        chunk_size_bytes,
        max_chunks,
        prefetch_chunks,
        memory_limit_bytes,
    })
}

pub(crate) async fn resolve_video_bench_movie(
    client: &crate::pcs::PcsClient,
    remote_path: &str,
) -> Result<VideoBenchMovie, String> {
    let normalized_path = normalize_remote_command_path(remote_path);
    if normalized_path == "/" {
        return Err("video_bench_path_must_be_file".to_string());
    }
    let parent = remote_parent_path_for_bench(normalized_path.as_str());
    let file_name = remote_file_name(normalized_path.as_str());
    let cancel = tokio_util::sync::CancellationToken::new();
    let entries = client
        .list(parent.as_str(), &cancel)
        .await
        .map_err(|err| format!("metadata_list_failed: {}", pcs_error_label(&err)))?;
    let entry = entries
        .into_iter()
        .find(|entry| entry.path == normalized_path || entry.server_filename == file_name)
        .ok_or_else(|| "metadata_entry_not_found".to_string())?;
    if entry.is_dir {
        return Err("video_bench_path_must_be_file".to_string());
    }
    let size_bytes = nonnegative_size(entry.size);
    if size_bytes == 0 {
        return Err("video_bench_file_size_empty".to_string());
    }
    let extension = Path::new(entry.server_filename.as_str())
        .extension()
        .and_then(|value| value.to_str())
        .map_or_else(String::new, str::to_ascii_lowercase);
    Ok(VideoBenchMovie {
        path_hash: redacted_path_hash(normalized_path.as_str()),
        entry,
        size_bytes,
        extension,
    })
}

pub(crate) fn remote_parent_path_for_bench(path: &str) -> String {
    let normalized = normalize_remote_command_path(path);
    let trimmed = normalized.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some(("", _)) | None => "/".to_string(),
        Some((parent, _)) => parent.to_string(),
    }
}

pub(crate) fn format_video_bench_start(
    options: &VideoBenchCliOptions,
    resolved: &VideoBenchResolvedOptions,
    movie: &VideoBenchMovie,
    account_uid: u64,
) -> serde_json::Value {
    let positions: Vec<String> = options
        .positions
        .iter()
        .map(|position| position.label())
        .collect();
    serde_json::json!({
        "type": "bench_start",
        "case": options.bench_case.as_str(),
        "redacted": options.redact,
        "account_hash": redacted_path_hash(&account_uid.to_string()),
        "movie": {
            "path_hash": movie.path_hash,
            "extension": movie.extension,
            "size_bytes": movie.size_bytes,
            "mtime": movie.entry.mtime,
        },
        "config": {
            "duration_seconds": options.duration_seconds,
            "chunk_size_bytes": resolved.chunk_size_bytes,
            "max_chunks": resolved.max_chunks,
            "prefetch_chunks": resolved.prefetch_chunks,
            "memory_limit_bytes": resolved.memory_limit_bytes,
            "reps": options.reps,
            "range_sizes": options.range_sizes,
            "positions": positions,
        },
    })
}

pub(crate) fn format_video_bench_end(options: &VideoBenchCliOptions) -> serde_json::Value {
    serde_json::json!({
        "type": "bench_end",
        "case": options.bench_case.as_str(),
        "redacted": options.redact,
    })
}

pub(crate) struct VideoBenchRangeFetch {
    pub(crate) bytes_len: usize,
    pub(crate) candidate_index: usize,
    pub(crate) refresh_count: usize,
}

pub(crate) async fn run_video_source_bench(
    output: &mut VideoBenchOutput,
    client: &crate::pcs::PcsClient,
    movie: &VideoBenchMovie,
    account_uid: u64,
    account_bduss: &str,
    options: &VideoBenchCliOptions,
) -> Result<(), String> {
    let locate_started = Instant::now();
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut urls = client
        .locate_download_urls(
            movie.entry.path.as_str(),
            account_uid,
            account_bduss,
            &cancel,
        )
        .await
        .map_err(|err| format!("locatedownload_failed: {}", pcs_error_label(&err)))?;
    let locate_elapsed = locate_started.elapsed();
    output.emit(&serde_json::json!({
        "type": "source_locatedownload",
        "path_hash": movie.path_hash,
        "duration_ms": locate_elapsed.as_millis(),
        "url_count": urls.len(),
        "ok": !urls.is_empty(),
    }))?;
    if urls.is_empty() {
        return Err("locatedownload_empty".to_string());
    }

    for &range_size in &options.range_sizes {
        let request_len = range_size.min(movie.size_bytes);
        if request_len == 0 {
            continue;
        }
        for &position in &options.positions {
            let offset = video_bench_offset(movie.size_bytes, request_len, position);
            for rep in 1..=options.reps {
                let end = offset.saturating_add(request_len).saturating_sub(1);
                let range =
                    crate::pcs::ByteRange::new(offset, end).map_err(|err| pcs_error_label(&err))?;
                let started = Instant::now();
                let result = fetch_video_bench_dlink_range(
                    client,
                    &mut urls,
                    movie.entry.path.as_str(),
                    account_uid,
                    account_bduss,
                    range,
                )
                .await;
                let elapsed = started.elapsed();
                match result {
                    Ok(fetch) => output.emit(&format_source_range_event(
                        movie,
                        position,
                        rep,
                        offset,
                        request_len,
                        &fetch,
                        elapsed,
                    ))?,
                    Err(error) => output.emit(&format_source_error_event(
                        movie,
                        position,
                        rep,
                        offset,
                        request_len,
                        elapsed,
                        error.as_str(),
                    ))?,
                }
            }
        }
    }

    Ok(())
}

pub(crate) async fn fetch_video_bench_dlink_range(
    client: &crate::pcs::PcsClient,
    urls: &mut Vec<String>,
    remote_path: &str,
    account_uid: u64,
    account_bduss: &str,
    range: crate::pcs::ByteRange,
) -> Result<VideoBenchRangeFetch, String> {
    match try_video_bench_dlink_candidates(client, urls, range).await {
        Ok(mut fetch) => {
            fetch.refresh_count = 0;
            Ok(fetch)
        }
        Err(_) => {
            let cancel = tokio_util::sync::CancellationToken::new();
            let refreshed = client
                .locate_download_urls(remote_path, account_uid, account_bduss, &cancel)
                .await
                .map_err(|err| {
                    format!("locatedownload_refresh_failed: {}", pcs_error_label(&err))
                })?;
            if refreshed.is_empty() {
                return Err("locatedownload_refresh_empty".to_string());
            }
            *urls = refreshed;
            let mut fetch = try_video_bench_dlink_candidates(client, urls, range).await?;
            fetch.refresh_count = 1;
            Ok(fetch)
        }
    }
}

pub(crate) async fn try_video_bench_dlink_candidates(
    client: &crate::pcs::PcsClient,
    urls: &[String],
    range: crate::pcs::ByteRange,
) -> Result<VideoBenchRangeFetch, String> {
    let mut last_error = "locatedownload_empty".to_string();
    for (index, url) in urls.iter().enumerate() {
        let cancel = tokio_util::sync::CancellationToken::new();
        match client
            .download_url_range(url.as_str(), range, &cancel)
            .await
        {
            Ok(bytes) => {
                return Ok(VideoBenchRangeFetch {
                    bytes_len: bytes.len(),
                    candidate_index: index,
                    refresh_count: 0,
                });
            }
            Err(err) => last_error = pcs_error_label(&err),
        }
    }
    Err(last_error)
}

pub(crate) fn format_source_range_event(
    movie: &VideoBenchMovie,
    position: VideoBenchPosition,
    rep: usize,
    offset: u64,
    request_len: u64,
    fetch: &VideoBenchRangeFetch,
    elapsed: Duration,
) -> serde_json::Value {
    let bytes_len = u64::try_from(fetch.bytes_len).unwrap_or(u64::MAX);
    serde_json::json!({
        "type": "source_range",
        "path_hash": movie.path_hash,
        "position": position.label(),
        "rep": rep,
        "offset": offset,
        "request_bytes": request_len,
        "bytes": fetch.bytes_len,
        "duration_ms": elapsed.as_millis(),
        "throughput_mib_s": rounded_throughput_mib_s(bytes_len, elapsed),
        "candidate_index": fetch.candidate_index,
        "refresh_count": fetch.refresh_count,
        "ok": bytes_len == request_len,
        "error": serde_json::Value::Null,
    })
}

pub(crate) fn format_source_error_event(
    movie: &VideoBenchMovie,
    position: VideoBenchPosition,
    rep: usize,
    offset: u64,
    request_len: u64,
    elapsed: Duration,
    error: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "source_range",
        "path_hash": movie.path_hash,
        "position": position.label(),
        "rep": rep,
        "offset": offset,
        "request_bytes": request_len,
        "bytes": 0,
        "duration_ms": elapsed.as_millis(),
        "throughput_mib_s": 0.0,
        "candidate_index": serde_json::Value::Null,
        "refresh_count": serde_json::Value::Null,
        "ok": false,
        "error": sanitize_video_bench_error(movie, error),
    })
}

pub(crate) fn video_bench_offset(
    file_size: u64,
    request_len: u64,
    position: VideoBenchPosition,
) -> u64 {
    let max_start = file_size.saturating_sub(request_len);
    match position {
        VideoBenchPosition::Start => 0,
        VideoBenchPosition::Percent(percent) => max_start.saturating_mul(u64::from(percent)) / 100,
        VideoBenchPosition::Tail => max_start,
    }
}

pub(crate) struct VideoBenchHttpResponse {
    pub(crate) status_code: u16,
    pub(crate) body_len: usize,
    pub(crate) content_length: Option<u64>,
    pub(crate) content_range: Option<String>,
}

pub(crate) async fn run_video_gateway_bench(
    output: &mut VideoBenchOutput,
    client: crate::pcs::PcsClient,
    movie: &VideoBenchMovie,
    account_uid: u64,
    account_bduss: String,
    resolved: &VideoBenchResolvedOptions,
    duration_seconds: u64,
) -> Result<(), String> {
    let budget =
        crate::preview::VideoCacheBudget::new(resolved.chunk_size_bytes, resolved.max_chunks)
            .map_err(|err| format!("video_budget_error: {err:?}"))?;
    let token = generate_video_token(1).map_err(|err| format!("video_token_error: {err:?}"))?;
    let upstream = Arc::new(PcsVideoUpstream::new(
        client,
        movie.entry.path.clone(),
        account_uid,
        account_bduss,
    ));
    let cancel = tokio_util::sync::CancellationToken::new();
    let server = crate::preview::VideoLoopbackServer::spawn_with_prefetch(
        nonnegative_size(movie.entry.size),
        token,
        budget,
        resolved.prefetch_chunks,
        upstream,
        cancel.clone(),
    )
    .await
    .map_err(|err| format!("video_server_error: {err:?}"))?;
    let gateway_client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|err| format!("gateway_client_failed: {}", error_chain_label(&err)))?;
    output.emit(&serde_json::json!({
        "type": "gateway_start",
        "path_hash": movie.path_hash,
        "port": server.port(),
        "chunk_size_bytes": resolved.chunk_size_bytes,
        "prefetch_chunks": resolved.prefetch_chunks,
        "duration_seconds": duration_seconds,
    }))?;

    let started = Instant::now();
    let deadline = Duration::from_secs(duration_seconds);
    let mut offset = 0_u64;
    let mut total_bytes = 0_u64;
    let mut requests = 0_u64;
    let mut stop_reason = "duration";
    while started.elapsed() < deadline && offset < movie.size_bytes {
        let request_len = resolved
            .chunk_size_bytes
            .min(movie.size_bytes.saturating_sub(offset));
        if request_len == 0 {
            stop_reason = "eof";
            break;
        }
        let end = offset.saturating_add(request_len).saturating_sub(1);
        let request_started = Instant::now();
        let response =
            video_bench_loopback_get(&gateway_client, server.local_url(), offset, end, &cancel)
                .await;
        let elapsed = request_started.elapsed();
        match response {
            Ok(response) => {
                let bytes_len = u64::try_from(response.body_len).unwrap_or(u64::MAX);
                let ok = response.status_code == 206
                    && bytes_len > 0
                    && response.content_length == Some(bytes_len)
                    && response.content_range.is_some();
                output.emit(&format_gateway_range_event(
                    movie,
                    offset,
                    request_len,
                    &response,
                    elapsed,
                    ok,
                    None,
                ))?;
                if !ok {
                    stop_reason = "http_error";
                    break;
                }
                requests = requests.saturating_add(1);
                total_bytes = total_bytes.saturating_add(bytes_len);
                offset = offset.saturating_add(bytes_len);
            }
            Err(error) => {
                output.emit(&format_gateway_range_error_event(
                    movie,
                    offset,
                    request_len,
                    elapsed,
                    error.as_str(),
                ))?;
                stop_reason = "error";
                break;
            }
        }
    }
    if offset >= movie.size_bytes {
        stop_reason = "eof";
    }

    let elapsed = started.elapsed();
    let stats = server.stats();
    output.emit(&format_gateway_summary_event(
        movie,
        requests,
        total_bytes,
        elapsed,
        &stats,
        stop_reason,
    ))
}

pub(crate) async fn video_bench_loopback_get(
    client: &reqwest::Client,
    url: &str,
    start: u64,
    end: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<VideoBenchHttpResponse, String> {
    let response = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("cancelled".to_string()),
        response = client
            .get(url)
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
            .send() => response.map_err(|err| {
                format!("gateway_request_failed: {}", error_chain_label(&err))
            })?,
    };
    let status_code = response.status().as_u16();
    let content_length = response.content_length();
    let content_range = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut body_len = 0_usize;
    let mut stream = response.bytes_stream();
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err("cancelled".to_string()),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk =
            chunk.map_err(|err| format!("gateway_body_failed: {}", error_chain_label(&err)))?;
        body_len = body_len
            .checked_add(chunk.len())
            .ok_or_else(|| "gateway_body_too_large".to_string())?;
    }
    Ok(VideoBenchHttpResponse {
        status_code,
        body_len,
        content_length,
        content_range,
    })
}

pub(crate) fn format_gateway_range_event(
    movie: &VideoBenchMovie,
    offset: u64,
    request_len: u64,
    response: &VideoBenchHttpResponse,
    elapsed: Duration,
    ok: bool,
    error: Option<&str>,
) -> serde_json::Value {
    let bytes_len = u64::try_from(response.body_len).unwrap_or(u64::MAX);
    serde_json::json!({
        "type": "gateway_range",
        "path_hash": movie.path_hash,
        "offset": offset,
        "request_bytes": request_len,
        "status": response.status_code,
        "content_length": response.content_length,
        "content_range": response.content_range,
        "bytes": response.body_len,
        "duration_ms": elapsed.as_millis(),
        "throughput_mib_s": rounded_throughput_mib_s(bytes_len, elapsed),
        "ok": ok,
        "error": error.map(|error| sanitize_video_bench_error(movie, error)),
    })
}

pub(crate) fn format_gateway_range_error_event(
    movie: &VideoBenchMovie,
    offset: u64,
    request_len: u64,
    elapsed: Duration,
    error: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "gateway_range",
        "path_hash": movie.path_hash,
        "offset": offset,
        "request_bytes": request_len,
        "status": serde_json::Value::Null,
        "content_length": serde_json::Value::Null,
        "content_range": serde_json::Value::Null,
        "bytes": 0,
        "duration_ms": elapsed.as_millis(),
        "throughput_mib_s": 0.0,
        "ok": false,
        "error": sanitize_video_bench_error(movie, error),
    })
}

pub(crate) fn format_gateway_summary_event(
    movie: &VideoBenchMovie,
    requests: u64,
    total_bytes: u64,
    elapsed: Duration,
    stats: &crate::preview::VideoProxyStats,
    stop_reason: &str,
) -> serde_json::Value {
    let last_error = stats
        .last_error
        .as_ref()
        .map(|err| sanitize_video_bench_error(movie, format!("{err:?}").as_str()));
    serde_json::json!({
        "type": "gateway_summary",
        "path_hash": movie.path_hash,
        "requests": requests,
        "bytes": total_bytes,
        "duration_ms": elapsed.as_millis(),
        "throughput_mib_s": rounded_throughput_mib_s(total_bytes, elapsed),
        "cache_hits": stats.cache_hits,
        "upstream_fetches": stats.upstream_fetches,
        "prefetch_fetches": stats.prefetch_fetches,
        "used_bytes": stats.used_bytes,
        "window_start": stats.window_start,
        "window_end": stats.window_end_inclusive,
        "stop_reason": stop_reason,
        "last_error": last_error,
    })
}

pub(crate) fn throughput_mib_s(bytes: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64();
    if seconds <= f64::EPSILON {
        0.0
    } else {
        bytes as f64 / 1024.0 / 1024.0 / seconds
    }
}

pub(crate) fn rounded_throughput_mib_s(bytes: u64, elapsed: Duration) -> f64 {
    (throughput_mib_s(bytes, elapsed) * 1_000.0).round() / 1_000.0
}

pub(crate) fn sanitize_video_bench_error(movie: &VideoBenchMovie, error: &str) -> String {
    let mut sanitized = error.to_string();
    for value in [
        movie.entry.path.as_str(),
        movie.entry.server_filename.as_str(),
    ] {
        if !value.is_empty() {
            sanitized = sanitized.replace(value, "<redacted-path>");
        }
    }
    crate::preview::sanitize_player_log_line(&sanitized)
}

pub(crate) fn redacted_path_hash(path: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}
