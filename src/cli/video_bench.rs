use super::*;

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

    pub(crate) fn emit(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.writer, "{line}").map_err(|err| io_error_label(&err))
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
    output.emit(format_video_bench_start(options, &resolved, &movie, user.uid).as_str())?;
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
    output.emit(format_video_bench_end(options).as_str())?;
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
) -> String {
    format!(
        "{{\"type\":\"bench_start\",\"case\":{},\"redacted\":{},\"account_hash\":{},\"movie\":{{\"path_hash\":{},\"extension\":{},\"size_bytes\":{},\"mtime\":{}}},\"config\":{{\"duration_seconds\":{},\"chunk_size_bytes\":{},\"max_chunks\":{},\"prefetch_chunks\":{},\"memory_limit_bytes\":{},\"reps\":{},\"range_sizes\":{},\"positions\":{}}}}}",
        json_string(options.bench_case.as_str()),
        options.redact,
        json_string(redacted_path_hash(&account_uid.to_string()).as_str()),
        json_string(movie.path_hash.as_str()),
        json_string(movie.extension.as_str()),
        movie.size_bytes,
        movie.entry.mtime,
        options.duration_seconds,
        resolved.chunk_size_bytes,
        resolved.max_chunks,
        resolved.prefetch_chunks,
        resolved.memory_limit_bytes,
        options.reps,
        json_u64_array(&options.range_sizes),
        json_position_array(&options.positions)
    )
}

pub(crate) fn format_video_bench_end(options: &VideoBenchCliOptions) -> String {
    format!(
        "{{\"type\":\"bench_end\",\"case\":{},\"redacted\":{}}}",
        json_string(options.bench_case.as_str()),
        options.redact
    )
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
    output.emit(
        format!(
            "{{\"type\":\"source_locatedownload\",\"path_hash\":{},\"duration_ms\":{},\"url_count\":{},\"ok\":{}}}",
            json_string(movie.path_hash.as_str()),
            locate_elapsed.as_millis(),
            urls.len(),
            !urls.is_empty()
        )
        .as_str(),
    )?;
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
                    Ok(fetch) => output.emit(
                        format_source_range_event(
                            movie,
                            position,
                            rep,
                            offset,
                            request_len,
                            &fetch,
                            elapsed,
                        )
                        .as_str(),
                    )?,
                    Err(error) => output.emit(
                        format_source_error_event(
                            movie,
                            position,
                            rep,
                            offset,
                            request_len,
                            elapsed,
                            error.as_str(),
                        )
                        .as_str(),
                    )?,
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
) -> String {
    let bytes_len = u64::try_from(fetch.bytes_len).unwrap_or(u64::MAX);
    format!(
        "{{\"type\":\"source_range\",\"path_hash\":{},\"position\":{},\"rep\":{},\"offset\":{},\"request_bytes\":{},\"bytes\":{},\"duration_ms\":{},\"throughput_mib_s\":{:.3},\"candidate_index\":{},\"refresh_count\":{},\"ok\":{},\"error\":null}}",
        json_string(movie.path_hash.as_str()),
        json_string(position.label().as_str()),
        rep,
        offset,
        request_len,
        fetch.bytes_len,
        elapsed.as_millis(),
        throughput_mib_s(bytes_len, elapsed),
        fetch.candidate_index,
        fetch.refresh_count,
        bytes_len == request_len
    )
}

pub(crate) fn format_source_error_event(
    movie: &VideoBenchMovie,
    position: VideoBenchPosition,
    rep: usize,
    offset: u64,
    request_len: u64,
    elapsed: Duration,
    error: &str,
) -> String {
    format!(
        "{{\"type\":\"source_range\",\"path_hash\":{},\"position\":{},\"rep\":{},\"offset\":{},\"request_bytes\":{},\"bytes\":0,\"duration_ms\":{},\"throughput_mib_s\":0.000,\"candidate_index\":null,\"refresh_count\":null,\"ok\":false,\"error\":{}}}",
        json_string(movie.path_hash.as_str()),
        json_string(position.label().as_str()),
        rep,
        offset,
        request_len,
        elapsed.as_millis(),
        json_string(error)
    )
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
        cancel,
    )
    .await
    .map_err(|err| format!("video_server_error: {err:?}"))?;
    let request_path = loopback_request_path(&server)?;
    output.emit(
        format!(
            "{{\"type\":\"gateway_start\",\"path_hash\":{},\"port\":{},\"chunk_size_bytes\":{},\"prefetch_chunks\":{},\"duration_seconds\":{}}}",
            json_string(movie.path_hash.as_str()),
            server.port(),
            resolved.chunk_size_bytes,
            resolved.prefetch_chunks,
            duration_seconds
        )
        .as_str(),
    )?;

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
        let response = video_bench_loopback_get(server.port(), request_path.as_str(), offset, end);
        let elapsed = request_started.elapsed();
        match response {
            Ok(response) => {
                let bytes_len = u64::try_from(response.body_len).unwrap_or(u64::MAX);
                let ok = response.status_code == 206
                    && bytes_len > 0
                    && response.content_length == Some(bytes_len)
                    && response.content_range.is_some();
                output.emit(
                    format_gateway_range_event(
                        movie,
                        offset,
                        request_len,
                        &response,
                        elapsed,
                        ok,
                        None,
                    )
                    .as_str(),
                )?;
                if !ok {
                    stop_reason = "http_error";
                    break;
                }
                requests = requests.saturating_add(1);
                total_bytes = total_bytes.saturating_add(bytes_len);
                offset = offset.saturating_add(bytes_len);
            }
            Err(error) => {
                output.emit(
                    format_gateway_range_error_event(
                        movie,
                        offset,
                        request_len,
                        elapsed,
                        error.as_str(),
                    )
                    .as_str(),
                )?;
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
    output.emit(
        format_gateway_summary_event(movie, requests, total_bytes, elapsed, &stats, stop_reason)
            .as_str(),
    )
}

pub(crate) fn loopback_request_path(
    server: &crate::preview::VideoLoopbackServer,
) -> Result<String, String> {
    let args = server
        .mpv_args_with_profile(crate::config::VideoPlayerProfile::Isolated)
        .map_err(|err| format!("video_loopback_url_error: {err:?}"))?;
    let url = args
        .iter()
        .find(|arg| arg.starts_with("http://127.0.0.1:"))
        .ok_or_else(|| "video_loopback_url_missing".to_string())?;
    parse_http_request_path(url)
}

pub(crate) fn parse_http_request_path(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "loopback_url_must_be_http".to_string())?;
    let slash = rest
        .find('/')
        .ok_or_else(|| "loopback_url_missing_path".to_string())?;
    Ok(rest[slash..].to_string())
}

pub(crate) fn video_bench_loopback_get(
    port: u16,
    request_path: &str,
    start: u64,
    end: u64,
) -> Result<VideoBenchHttpResponse, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|err| io_error_label(&err))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|err| io_error_label(&err))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|err| io_error_label(&err))?;
    let request = format!(
        "GET {request_path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nRange: bytes={start}-{end}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| io_error_label(&err))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|err| io_error_label(&err))?;
    parse_video_bench_http_response(&response)
}

pub(crate) fn parse_video_bench_http_response(
    data: &[u8],
) -> Result<VideoBenchHttpResponse, String> {
    let header_end = data
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "http_response_missing_header_end".to_string())?;
    let body_start = header_end.saturating_add(4);
    let header = String::from_utf8_lossy(&data[..header_end]);
    let mut lines = header.split("\r\n");
    let status_code = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "http_response_bad_status".to_string())?;
    let mut content_length = None;
    let mut content_range = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<u64>().ok();
        } else if name.trim().eq_ignore_ascii_case("content-range") {
            content_range = Some(value.trim().to_string());
        }
    }
    Ok(VideoBenchHttpResponse {
        status_code,
        body_len: data.len().saturating_sub(body_start),
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
) -> String {
    let bytes_len = u64::try_from(response.body_len).unwrap_or(u64::MAX);
    format!(
        "{{\"type\":\"gateway_range\",\"path_hash\":{},\"offset\":{},\"request_bytes\":{},\"status\":{},\"content_length\":{},\"content_range\":{},\"bytes\":{},\"duration_ms\":{},\"throughput_mib_s\":{:.3},\"ok\":{},\"error\":{}}}",
        json_string(movie.path_hash.as_str()),
        offset,
        request_len,
        response.status_code,
        json_optional_u64(response.content_length),
        json_optional_string(response.content_range.as_deref()),
        response.body_len,
        elapsed.as_millis(),
        throughput_mib_s(bytes_len, elapsed),
        ok,
        json_optional_string(error)
    )
}

pub(crate) fn format_gateway_range_error_event(
    movie: &VideoBenchMovie,
    offset: u64,
    request_len: u64,
    elapsed: Duration,
    error: &str,
) -> String {
    format!(
        "{{\"type\":\"gateway_range\",\"path_hash\":{},\"offset\":{},\"request_bytes\":{},\"status\":null,\"content_length\":null,\"content_range\":null,\"bytes\":0,\"duration_ms\":{},\"throughput_mib_s\":0.000,\"ok\":false,\"error\":{}}}",
        json_string(movie.path_hash.as_str()),
        offset,
        request_len,
        elapsed.as_millis(),
        json_string(error)
    )
}

pub(crate) fn format_gateway_summary_event(
    movie: &VideoBenchMovie,
    requests: u64,
    total_bytes: u64,
    elapsed: Duration,
    stats: &crate::preview::VideoProxyStats,
    stop_reason: &str,
) -> String {
    let last_error = stats.last_error.as_ref().map(|err| format!("{err:?}"));
    format!(
        "{{\"type\":\"gateway_summary\",\"path_hash\":{},\"requests\":{},\"bytes\":{},\"duration_ms\":{},\"throughput_mib_s\":{:.3},\"cache_hits\":{},\"upstream_fetches\":{},\"prefetch_fetches\":{},\"used_bytes\":{},\"window_start\":{},\"window_end\":{},\"stop_reason\":{},\"last_error\":{}}}",
        json_string(movie.path_hash.as_str()),
        requests,
        total_bytes,
        elapsed.as_millis(),
        throughput_mib_s(total_bytes, elapsed),
        stats.cache_hits,
        stats.upstream_fetches,
        stats.prefetch_fetches,
        stats.used_bytes,
        json_optional_u64(stats.window_start),
        json_optional_u64(stats.window_end_inclusive),
        json_string(stop_reason),
        json_optional_string(last_error.as_deref())
    )
}

pub(crate) fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len().saturating_add(2));
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                let _ = write!(&mut escaped, "\\u{:04x}", u32::from(ch));
            }
            ch => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

pub(crate) fn json_optional_string(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_string(), json_string)
}

pub(crate) fn json_optional_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_string(), |value| value.to_string())
}

pub(crate) fn json_u64_array(values: &[u64]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(&mut out, "{value}");
    }
    out.push(']');
    out
}

pub(crate) fn json_position_array(values: &[VideoBenchPosition]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(json_string(value.label().as_str()).as_str());
    }
    out.push(']');
    out
}

pub(crate) fn throughput_mib_s(bytes: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64();
    if seconds <= f64::EPSILON {
        0.0
    } else {
        bytes as f64 / 1024.0 / 1024.0 / seconds
    }
}

pub(crate) fn redacted_path_hash(path: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}
