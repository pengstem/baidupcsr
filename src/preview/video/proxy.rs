use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VideoProxyMethod {
    Head,
    Get,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VideoProxyRequest {
    pub(crate) method: VideoProxyMethod,
    pub(crate) token: String,
    pub(crate) range: Option<RequestedVideoRange>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VideoProxyStats {
    pub used_bytes: u64,
    pub cache_hits: u64,
    pub upstream_fetches: u64,
    pub prefetch_fetches: u64,
    pub last_range_start: Option<u64>,
    pub last_range_end_inclusive: Option<u64>,
    pub window_start: Option<u64>,
    pub window_end_inclusive: Option<u64>,
    pub last_error: Option<VideoError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerLogRing {
    pub(crate) limit: usize,
    pub(crate) lines: VecDeque<String>,
}

impl PlayerLogRing {
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            lines: VecDeque::new(),
        }
    }

    pub fn push(&mut self, line: &str) {
        if self.limit == 0 {
            return;
        }
        while self.lines.len() >= self.limit {
            self.lines.pop_front();
        }
        self.lines.push_back(sanitize_player_log_line(line));
    }

    #[cfg(test)]
    pub(crate) fn lines(&self) -> Vec<&str> {
        self.lines.iter().map(String::as_str).collect()
    }

    pub fn last_line(&self) -> Option<&str> {
        self.lines.back().map(String::as_str)
    }
}

pub(crate) fn sanitize_player_log_line(line: &str) -> String {
    line.split_whitespace()
        .map(sanitize_player_log_part)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn sanitize_player_log_part(part: &str) -> String {
    if part.starts_with("http://") || part.starts_with("https://") {
        return "<redacted-url>".to_string();
    }
    let Some((key, _value)) = part.split_once('=') else {
        return part.to_string();
    };
    let normalized = key
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .to_ascii_lowercase();
    if is_sensitive_log_key(normalized.as_str()) {
        format!("{key}=<redacted>")
    } else {
        part.to_string()
    }
}

pub(crate) fn is_sensitive_log_key(key: &str) -> bool {
    key.contains("token")
        || matches!(
            key,
            "bduss" | "cookie" | "cookies" | "authorization" | "auth" | "sign" | "secret"
        )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MpvLaunchProfile {
    User,
    Isolated,
}

#[derive(Clone, Debug)]
pub(crate) struct VideoPreviewSession {
    pub(crate) key: PreviewKey,
    pub(crate) token: VideoToken,
    pub(crate) port: u16,
    pub(crate) budget: VideoCacheBudget,
}

impl VideoPreviewSession {
    pub(crate) const fn new(
        key: PreviewKey,
        token: VideoToken,
        port: u16,
        budget: VideoCacheBudget,
    ) -> Self {
        Self {
            key,
            token,
            port,
            budget,
        }
    }

    pub(crate) fn player_url(&self) -> String {
        format!(
            "http://{}:{}{}{}",
            LOOPBACK_HOST,
            self.port,
            VIDEO_PREVIEW_PATH,
            self.token.as_str()
        )
    }

    pub(crate) fn mpv_args_with_profile(&self, profile: MpvLaunchProfile) -> Vec<String> {
        let mut args = match profile {
            MpvLaunchProfile::User => vec![
                "--no-terminal".to_string(),
                "--cache-on-disk=no".to_string(),
                "--cache=yes".to_string(),
                "--cache-pause=no".to_string(),
                "--demuxer-readahead-secs=20".to_string(),
            ],
            MpvLaunchProfile::Isolated => vec![
                "--no-config".to_string(),
                "--load-scripts=no".to_string(),
                "--osc=no".to_string(),
                "--no-terminal".to_string(),
                "--cache-on-disk=no".to_string(),
                "--cache=yes".to_string(),
            ],
        };
        args.extend([
            format!("--demuxer-max-bytes={}", self.budget.memory_limit_bytes()),
            "--demuxer-max-back-bytes=0".to_string(),
            "--force-window=immediate".to_string(),
        ]);
        if profile == MpvLaunchProfile::Isolated {
            args.extend(["--ao=null".to_string(), "--mute=yes".to_string()]);
        }
        args.extend([
            "--idle=yes".to_string(),
            "--msg-level=all=warn".to_string(),
            "--no-ytdl".to_string(),
            self.player_url(),
        ]);
        args
    }

    pub(crate) fn default_range(&self) -> VideoRange {
        let end = self
            .budget
            .chunk_size_bytes
            .saturating_sub(1)
            .min(self.key.size.saturating_sub(1));
        VideoRange {
            start: 0,
            end_inclusive: end,
        }
    }
}

use std::future::Future;
use std::pin::Pin;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex as TokioMutex, Notify, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

pub trait VideoUpstream: Send + Sync {
    fn fetch<'a>(
        &'a self,
        range: VideoRange,
        cancel: &'a CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, VideoError>> + Send + 'a>>;
}

pub(crate) struct AsyncVideoState {
    pub(crate) cache: VideoIntervalBuffer,
    pub(crate) inflight: HashMap<VideoRange, Arc<Notify>>,
    pub(crate) stats: VideoProxyStats,
}

pub struct VideoLoopbackServer {
    #[allow(dead_code, reason = "exposed to loopback integration tests")]
    pub(crate) local_url: String,
    pub(crate) session: VideoPreviewSession,
    pub(crate) cancel: CancellationToken,
    pub(crate) stats: watch::Receiver<VideoProxyStats>,
    pub(crate) handle: Option<JoinHandle<Result<(), VideoError>>>,
}

impl VideoLoopbackServer {
    pub async fn spawn_with_prefetch<U: VideoUpstream + 'static>(
        content_length: u64,
        token: VideoToken,
        budget: VideoCacheBudget,
        prefetch_chunks: usize,
        upstream: Arc<U>,
        cancel: CancellationToken,
    ) -> Result<Self, VideoError> {
        let listener = TcpListener::bind((LOOPBACK_HOST, 0))
            .await
            .map_err(io_video_error)?;
        let port = listener.local_addr().map_err(io_video_error)?.port();
        let key = PreviewKey {
            account_uid: 0,
            fs_id: 0,
            mtime: 0,
            size: content_length,
        };
        let session = VideoPreviewSession::new(key, token, port, budget);
        let local_url = session.player_url();
        let state = Arc::new(TokioMutex::new(AsyncVideoState {
            cache: VideoIntervalBuffer::new(budget),
            inflight: HashMap::new(),
            stats: VideoProxyStats::default(),
        }));
        let (stats_tx, stats_rx) = watch::channel(VideoProxyStats::default());
        let semaphore = Arc::new(Semaphore::new(prefetch_chunks.max(1)));
        let session_for_task = session.clone();
        let cancel_for_task = cancel.clone();
        let upstream: Arc<dyn VideoUpstream> = upstream;
        let handle = tokio::spawn(async move {
            serve_async_video(
                listener,
                session_for_task,
                state,
                stats_tx,
                upstream,
                semaphore,
                prefetch_chunks,
                cancel_for_task,
            )
            .await
        });
        Ok(Self {
            local_url,
            session,
            cancel,
            stats: stats_rx,
            handle: Some(handle),
        })
    }

    #[allow(dead_code, reason = "exposed to loopback integration tests")]
    pub fn local_url(&self) -> &str {
        &self.local_url
    }

    pub const fn port(&self) -> u16 {
        self.session.port
    }

    pub fn stats(&self) -> VideoProxyStats {
        self.stats.borrow().clone()
    }

    pub fn mpv_args_with_profile(
        &self,
        profile: MpvLaunchProfile,
    ) -> Result<Vec<String>, VideoError> {
        Ok(self.session.mpv_args_with_profile(profile))
    }

    pub async fn shutdown(mut self) -> Result<(), VideoError> {
        self.cancel.cancel();
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        match handle.await {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(VideoError::Io(error.to_string())),
        }
    }
}

impl Drop for VideoLoopbackServer {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

// Connection context is explicit so cancellation and resource limits cannot drift apart.
#[expect(clippy::too_many_arguments)]
pub(crate) async fn serve_async_video(
    listener: TcpListener,
    session: VideoPreviewSession,
    state: Arc<TokioMutex<AsyncVideoState>>,
    stats_tx: watch::Sender<VideoProxyStats>,
    upstream: Arc<dyn VideoUpstream>,
    semaphore: Arc<Semaphore>,
    prefetch_chunks: usize,
    cancel: CancellationToken,
) -> Result<(), VideoError> {
    let mut connections = JoinSet::new();
    loop {
        let has_connections = !connections.is_empty();
        let accepted = tokio::select! {
            _ = cancel.cancelled() => {
                connections.abort_all();
                while connections.join_next().await.is_some() {}
                return Ok(());
            },
            _ = connections.join_next(), if has_connections => continue,
            accepted = listener.accept() => accepted.map_err(io_video_error)?,
        };
        let (stream, _) = accepted;
        let session = session.clone();
        let state = Arc::clone(&state);
        let stats_tx = stats_tx.clone();
        let upstream = Arc::clone(&upstream);
        let semaphore = Arc::clone(&semaphore);
        let cancel = cancel.child_token();
        connections.spawn(async move {
            let _ = handle_async_connection(
                stream,
                session,
                state,
                stats_tx,
                upstream,
                semaphore,
                prefetch_chunks,
                cancel,
            )
            .await;
        });
    }
}

// Connection context is explicit so cancellation and resource limits cannot drift apart.
#[expect(clippy::too_many_arguments)]
pub(crate) async fn handle_async_connection(
    mut stream: TcpStream,
    session: VideoPreviewSession,
    state: Arc<TokioMutex<AsyncVideoState>>,
    stats_tx: watch::Sender<VideoProxyStats>,
    upstream: Arc<dyn VideoUpstream>,
    semaphore: Arc<Semaphore>,
    prefetch_chunks: usize,
    cancel: CancellationToken,
) -> Result<(), VideoError> {
    let request = read_async_request(&mut stream, &cancel).await?;
    if !session.token.matches(&request.token) {
        write_http_error(&mut stream, 403, "Forbidden").await?;
        return Err(VideoError::InvalidToken);
    }
    if request.method == VideoProxyMethod::Head {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
            session.key.size
        );
        stream
            .write_all(response.as_bytes())
            .await
            .map_err(io_video_error)?;
        return Ok(());
    }

    let requested = request
        .range
        .unwrap_or_else(|| session.default_range().into());
    let max_len = match request.range {
        Some(RequestedVideoRange::Explicit(_)) => session.budget.memory_limit_bytes(),
        _ => session.budget.chunk_size_bytes,
    };
    let range = requested
        .resolve(session.key.size, max_len)
        .map_err(|_| VideoError::InvalidRange)?;
    let bytes = cached_fetch(
        range, &state, &stats_tx, &upstream, &semaphore, &cancel, false,
    )
    .await?;
    let response = format!(
        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        bytes.len(), range.start, range.end_inclusive, session.key.size
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(io_video_error)?;
    stream.write_all(&bytes).await.map_err(io_video_error)?;

    for index in 1..=prefetch_chunks {
        let start = range.end_inclusive.saturating_add(1).saturating_add(
            session
                .budget
                .chunk_size_bytes
                .saturating_mul(u64::try_from(index.saturating_sub(1)).unwrap_or(u64::MAX)),
        );
        if start >= session.key.size {
            break;
        }
        let end = start
            .saturating_add(session.budget.chunk_size_bytes)
            .saturating_sub(1)
            .min(session.key.size.saturating_sub(1));
        let prefetch_range = VideoRange {
            start,
            end_inclusive: end,
        };
        let state = Arc::clone(&state);
        let stats_tx = stats_tx.clone();
        let upstream = Arc::clone(&upstream);
        let semaphore = Arc::clone(&semaphore);
        let cancel = cancel.child_token();
        tokio::spawn(async move {
            let _ = cached_fetch(
                prefetch_range,
                &state,
                &stats_tx,
                &upstream,
                &semaphore,
                &cancel,
                true,
            )
            .await;
        });
    }
    Ok(())
}

pub(crate) async fn cached_fetch(
    range: VideoRange,
    state: &Arc<TokioMutex<AsyncVideoState>>,
    stats_tx: &watch::Sender<VideoProxyStats>,
    upstream: &Arc<dyn VideoUpstream>,
    semaphore: &Arc<Semaphore>,
    cancel: &CancellationToken,
    prefetch: bool,
) -> Result<Vec<u8>, VideoError> {
    loop {
        let wait =
            {
                let mut state = state.lock().await;
                if let Some(bytes) = state.cache.get(range) {
                    state.stats.cache_hits = state.stats.cache_hits.saturating_add(1);
                    update_stats(&mut state);
                    let _ = stats_tx.send(state.stats.clone());
                    return Ok(bytes);
                }
                if let Some(notify) = state.inflight.iter().find_map(|(inflight, notify)| {
                    inflight.contains(range).then(|| Arc::clone(notify))
                }) {
                    let mut notified = Box::pin(notify.notified_owned());
                    notified.as_mut().enable();
                    Some(notified)
                } else {
                    state.inflight.insert(range, Arc::new(Notify::new()));
                    None
                }
            };
        if let Some(notify) = wait {
            tokio::select! {
                _ = cancel.cancelled() => return Err(VideoError::Cancelled),
                _ = notify => continue,
            }
        }
        break;
    }

    let permit = tokio::select! {
        _ = cancel.cancelled() => return finish_fetch_error(range, state, stats_tx, VideoError::Cancelled).await,
        permit = semaphore.acquire() => permit.map_err(|_| VideoError::Cancelled)?,
    };
    let fetched = upstream.fetch(range, cancel).await;
    drop(permit);
    match fetched {
        Ok(bytes) => {
            let mut state = state.lock().await;
            let notify = state.inflight.remove(&range);
            state
                .cache
                .put(range, bytes.clone())
                .map_err(|error| VideoError::Upstream(format!("{error:?}")))?;
            state.stats.upstream_fetches = state.stats.upstream_fetches.saturating_add(1);
            if prefetch {
                state.stats.prefetch_fetches = state.stats.prefetch_fetches.saturating_add(1);
            }
            state.stats.last_range_start = Some(range.start);
            state.stats.last_range_end_inclusive = Some(range.end_inclusive);
            update_stats(&mut state);
            let _ = stats_tx.send(state.stats.clone());
            if let Some(notify) = notify {
                notify.notify_waiters();
            }
            Ok(bytes)
        }
        Err(error) => finish_fetch_error(range, state, stats_tx, error).await,
    }
}

pub(crate) async fn finish_fetch_error(
    range: VideoRange,
    state: &Arc<TokioMutex<AsyncVideoState>>,
    stats_tx: &watch::Sender<VideoProxyStats>,
    error: VideoError,
) -> Result<Vec<u8>, VideoError> {
    let mut state = state.lock().await;
    let notify = state.inflight.remove(&range);
    state.stats.last_error = Some(error.clone());
    let _ = stats_tx.send(state.stats.clone());
    if let Some(notify) = notify {
        notify.notify_waiters();
    }
    Err(error)
}

pub(crate) fn update_stats(state: &mut AsyncVideoState) {
    state.stats.used_bytes = state.cache.used_bytes;
    if let Some(window) = state.cache.window() {
        state.stats.window_start = Some(window.start);
        state.stats.window_end_inclusive = Some(window.end_inclusive);
    }
}

pub(crate) async fn read_async_request(
    stream: &mut TcpStream,
    cancel: &CancellationToken,
) -> Result<VideoProxyRequest, VideoError> {
    let mut data = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    while data.len() < 16 * 1024 {
        let read = tokio::select! {
            _ = cancel.cancelled() => return Err(VideoError::Cancelled),
            read = stream.read(&mut buffer) => read.map_err(io_video_error)?,
        };
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..read]);
        if data.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let text = std::str::from_utf8(&data).map_err(|_| VideoError::MalformedRequest)?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or(VideoError::MalformedRequest)?;
    let mut parts = request_line.split_whitespace();
    let method = match parts.next() {
        Some("HEAD") => VideoProxyMethod::Head,
        Some("GET") => VideoProxyMethod::Get,
        _ => return Err(VideoError::UnsupportedMethod),
    };
    let path = parts.next().ok_or(VideoError::MalformedRequest)?;
    let token = path
        .strip_prefix(VIDEO_PREVIEW_PATH)
        .ok_or(VideoError::InvalidToken)?
        .to_string();
    let mut range = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("range") {
            range = Some(parse_async_range(value.trim())?);
        }
    }
    Ok(VideoProxyRequest {
        method,
        token,
        range,
    })
}

pub(crate) fn parse_async_range(value: &str) -> Result<RequestedVideoRange, VideoError> {
    let value = value
        .strip_prefix("bytes=")
        .ok_or(VideoError::InvalidRange)?;
    let (start, end) = value.split_once('-').ok_or(VideoError::InvalidRange)?;
    if start.is_empty() {
        let bytes = end.parse::<u64>().map_err(|_| VideoError::InvalidRange)?;
        return RequestedVideoRange::suffix(bytes).map_err(|_| VideoError::InvalidRange);
    }
    let start = start.parse::<u64>().map_err(|_| VideoError::InvalidRange)?;
    if end.is_empty() {
        return Ok(RequestedVideoRange::OpenEnded(start));
    }
    let end = end.parse::<u64>().map_err(|_| VideoError::InvalidRange)?;
    VideoRange::new(start, end)
        .map(RequestedVideoRange::Explicit)
        .map_err(|_| VideoError::InvalidRange)
}

pub(crate) async fn write_http_error(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
) -> Result<(), VideoError> {
    let response =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(io_video_error)
}

pub(crate) fn io_video_error(error: std::io::Error) -> VideoError {
    VideoError::Io(error.to_string())
}
