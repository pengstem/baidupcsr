use std::{
    collections::{HashMap, VecDeque},
    fmt,
};

pub const fn crate_name() -> &'static str {
    "baidupcsr-preview"
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PreviewKey {
    pub account_uid: u64,
    pub fs_id: i64,
    pub mtime: i64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewKind {
    Text,
    Image,
    Metadata,
    Video,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewAction {
    FetchText,
    MetadataOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewPlan {
    pub kind: PreviewKind,
    pub action: PreviewAction,
    pub reason: String,
}

impl PreviewPlan {
    pub fn for_entry(
        file_name: &str,
        is_dir: bool,
        size_bytes: u64,
        memory_limit_bytes: u64,
    ) -> Self {
        if is_dir {
            return Self::metadata(PreviewKind::Metadata, "directory metadata only");
        }
        if size_bytes > memory_limit_bytes {
            return Self::metadata(PreviewKind::Metadata, "file exceeds preview memory budget");
        }

        match preview_kind_from_name(file_name) {
            PreviewKind::Text => Self {
                kind: PreviewKind::Text,
                action: PreviewAction::FetchText,
                reason: "text preview within memory budget".to_string(),
            },
            kind => Self::metadata(kind, "binary or unsupported preview type"),
        }
    }

    fn metadata(kind: PreviewKind, reason: &str) -> Self {
        Self {
            kind,
            action: PreviewAction::MetadataOnly,
            reason: reason.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviewStatus {
    pub kind: PreviewKind,
    pub fetched_bytes: u64,
    pub memory_limit_bytes: u64,
    pub truncated: bool,
    pub disk_backed: bool,
}

pub fn preview_kind_from_name(file_name: &str) -> PreviewKind {
    let Some(extension) = file_name.rsplit_once('.').map(|(_, extension)| extension) else {
        return PreviewKind::Metadata;
    };

    match extension.to_ascii_lowercase().as_str() {
        "txt" | "md" | "markdown" | "rs" | "toml" | "json" | "yaml" | "yml" | "log" | "c"
        | "cc" | "cpp" | "h" | "hpp" | "py" | "js" | "ts" | "tsx" | "jsx" | "html" | "css"
        | "xml" | "csv" | "ini" | "conf" | "sh" | "bash" | "zsh" | "fish" => PreviewKind::Text,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "avif" => PreviewKind::Image,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "flv" => PreviewKind::Video,
        _ => PreviewKind::Metadata,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewText {
    pub content: String,
    pub status: PreviewStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestState {
    Applied,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviewRequest {
    key: PreviewKey,
    generation: u64,
}

impl PreviewRequest {
    pub const fn key(self) -> PreviewKey {
        self.key
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviewUpdate {
    pub status: PreviewStatus,
    pub request_state: RequestState,
}

#[derive(Clone, Debug)]
pub struct PreviewManager {
    memory_limit_bytes: u64,
    used_bytes: u64,
    cache: HashMap<PreviewKey, PreviewText>,
    lru: VecDeque<PreviewKey>,
    generations: HashMap<PreviewKey, u64>,
}

impl PreviewManager {
    pub fn new(memory_limit_bytes: u64) -> Self {
        Self {
            memory_limit_bytes,
            used_bytes: 0,
            cache: HashMap::new(),
            lru: VecDeque::new(),
            generations: HashMap::new(),
        }
    }

    pub fn start_request(&mut self, key: PreviewKey) -> PreviewRequest {
        let generation = self
            .generations
            .get(&key)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        self.generations.insert(key, generation);
        PreviewRequest { key, generation }
    }

    pub fn finish_text_request(&mut self, request: PreviewRequest, bytes: &[u8]) -> PreviewUpdate {
        let status = self.text_status(bytes);
        if self.generations.get(&request.key).copied() != Some(request.generation) {
            return PreviewUpdate {
                status,
                request_state: RequestState::Stale,
            };
        }

        let content_len = usize::try_from(status.fetched_bytes)
            .unwrap_or(bytes.len())
            .min(bytes.len());
        let content = String::from_utf8_lossy(&bytes[..content_len]).to_string();
        self.insert(request.key, PreviewText { content, status });

        PreviewUpdate {
            status,
            request_state: RequestState::Applied,
        }
    }

    pub fn get(&self, key: &PreviewKey) -> Option<&PreviewText> {
        self.cache.get(key)
    }

    pub const fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    fn text_status(&self, bytes: &[u8]) -> PreviewStatus {
        let incoming = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let fetched = incoming.min(self.memory_limit_bytes);
        PreviewStatus {
            kind: PreviewKind::Text,
            fetched_bytes: fetched,
            memory_limit_bytes: self.memory_limit_bytes,
            truncated: incoming > fetched,
            disk_backed: false,
        }
    }

    fn insert(&mut self, key: PreviewKey, text: PreviewText) {
        if let Some(old) = self.cache.remove(&key) {
            self.used_bytes = self.used_bytes.saturating_sub(old.status.fetched_bytes);
        }
        self.lru.retain(|item| item != &key);

        self.used_bytes = self.used_bytes.saturating_add(text.status.fetched_bytes);
        self.cache.insert(key, text);
        self.lru.push_back(key);
        self.evict_until_within_budget();
    }

    fn evict_until_within_budget(&mut self) {
        while self.used_bytes > self.memory_limit_bytes {
            let Some(key) = self.lru.pop_front() else {
                break;
            };
            if let Some(old) = self.cache.remove(&key) {
                self.used_bytes = self.used_bytes.saturating_sub(old.status.fetched_bytes);
            }
        }
    }
}

const LOOPBACK_HOST: &str = "127.0.0.1";
const VIDEO_PREVIEW_PATH: &str = "/preview/video/";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VideoPreviewError {
    InvalidUpstreamUrl,
    InvalidVideoTokenFormat,
    InvalidCacheBudget,
    InvalidRange,
    InvalidPort,
    NonLoopbackBindAddress,
    InvalidVideoToken,
    InactiveVideoSession,
    VideoChunkTooLarge,
}

#[derive(Clone, Eq, PartialEq)]
pub struct VideoUpstream {
    url: String,
}

impl VideoUpstream {
    pub fn new(url: impl Into<String>) -> Result<Self, VideoPreviewError> {
        let url = url.into();
        if url.starts_with("https://") || url.starts_with("http://") {
            Ok(Self { url })
        } else {
            Err(VideoPreviewError::InvalidUpstreamUrl)
        }
    }

    fn as_str(&self) -> &str {
        &self.url
    }
}

impl fmt::Debug for VideoUpstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VideoUpstream")
            .field("url", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoToken {
    token: String,
}

impl VideoToken {
    pub fn new(token: impl Into<String>) -> Result<Self, VideoPreviewError> {
        let token = token.into();
        let is_url_safe = token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        if token.len() < 16 || !is_url_safe {
            return Err(VideoPreviewError::InvalidVideoTokenFormat);
        }

        Ok(Self { token })
    }

    pub fn as_str(&self) -> &str {
        &self.token
    }

    fn matches(&self, candidate: &str) -> bool {
        self.token == candidate
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoopbackEndpoint {
    port: u16,
}

impl LoopbackEndpoint {
    pub fn new(port: u16) -> Result<Self, VideoPreviewError> {
        Self::from_host_port(LOOPBACK_HOST, port)
    }

    pub fn from_host_port(host: &str, port: u16) -> Result<Self, VideoPreviewError> {
        if host != LOOPBACK_HOST {
            return Err(VideoPreviewError::NonLoopbackBindAddress);
        }
        if port == 0 {
            return Err(VideoPreviewError::InvalidPort);
        }
        Ok(Self { port })
    }

    pub const fn host(&self) -> &'static str {
        LOOPBACK_HOST
    }

    pub const fn port(&self) -> u16 {
        self.port
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoCacheBudget {
    pub chunk_size_bytes: u64,
    pub max_chunks: usize,
}

impl VideoCacheBudget {
    pub fn new(chunk_size_bytes: u64, max_chunks: usize) -> Result<Self, VideoPreviewError> {
        if chunk_size_bytes == 0 || max_chunks == 0 {
            return Err(VideoPreviewError::InvalidCacheBudget);
        }

        Ok(Self {
            chunk_size_bytes,
            max_chunks,
        })
    }

    pub fn memory_limit_bytes(&self) -> u64 {
        self.chunk_size_bytes
            .saturating_mul(u64::try_from(self.max_chunks).unwrap_or(u64::MAX))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoRange {
    pub start: u64,
    pub end_inclusive: u64,
}

impl VideoRange {
    pub fn new(start: u64, end_inclusive: u64) -> Result<Self, VideoPreviewError> {
        if start > end_inclusive {
            return Err(VideoPreviewError::InvalidRange);
        }
        Ok(Self {
            start,
            end_inclusive,
        })
    }

    pub fn header_value(&self) -> String {
        format!("bytes={}-{}", self.start, self.end_inclusive)
    }

    fn clamp_to_file(self, file_size: u64) -> Result<Self, VideoPreviewError> {
        if file_size == 0 || self.start >= file_size {
            return Err(VideoPreviewError::InvalidRange);
        }

        Ok(Self {
            start: self.start,
            end_inclusive: self.end_inclusive.min(file_size - 1),
        })
    }
}

#[derive(Clone, Debug)]
pub struct VideoChunkCache {
    budget: VideoCacheBudget,
    used_bytes: u64,
    chunks: HashMap<u64, Vec<u8>>,
    lru: VecDeque<u64>,
}

impl VideoChunkCache {
    pub fn new(budget: VideoCacheBudget) -> Self {
        Self {
            budget,
            used_bytes: 0,
            chunks: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    pub fn put(&mut self, offset: u64, bytes: Vec<u8>) -> Result<(), VideoPreviewError> {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if len > self.budget.chunk_size_bytes {
            return Err(VideoPreviewError::VideoChunkTooLarge);
        }

        self.remove(offset);
        self.used_bytes = self.used_bytes.saturating_add(len);
        self.chunks.insert(offset, bytes);
        self.touch(offset);
        self.evict_until_within_budget();
        Ok(())
    }

    pub fn get(&mut self, offset: u64) -> Option<&[u8]> {
        if !self.chunks.contains_key(&offset) {
            return None;
        }
        self.touch(offset);
        self.chunks.get(&offset).map(Vec::as_slice)
    }

    pub const fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    pub fn retain_window(&mut self, window_start: u64, window_len: u64) {
        let window_end = window_start.saturating_add(window_len);
        let stale_offsets: Vec<u64> = self
            .chunks
            .iter()
            .filter_map(|(offset, bytes)| {
                let chunk_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                let chunk_end = offset.saturating_add(chunk_len);
                if chunk_end <= window_start || *offset >= window_end {
                    Some(*offset)
                } else {
                    None
                }
            })
            .collect();

        for offset in stale_offsets {
            self.remove(offset);
        }
    }

    fn remove(&mut self, offset: u64) {
        if let Some(bytes) = self.chunks.remove(&offset) {
            let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            self.used_bytes = self.used_bytes.saturating_sub(len);
        }
        self.lru.retain(|item| item != &offset);
    }

    fn touch(&mut self, offset: u64) {
        self.lru.retain(|item| item != &offset);
        self.lru.push_back(offset);
    }

    fn evict_until_within_budget(&mut self) {
        while self.used_bytes > self.budget.memory_limit_bytes() {
            let Some(offset) = self.lru.pop_front() else {
                break;
            };
            if let Some(bytes) = self.chunks.remove(&offset) {
                let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                self.used_bytes = self.used_bytes.saturating_sub(len);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoProxyMethod {
    Head,
    Get,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoProxyRequest {
    pub method: VideoProxyMethod,
    pub token: String,
    pub range: Option<VideoRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoProxyResponse {
    Head {
        content_length: u64,
        accept_ranges: bool,
    },
    Fetch(VideoRange),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerBackend {
    Mpv,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerLaunch {
    program: String,
    args: Vec<String>,
    max_memory_cache_bytes: u64,
    disk_cache_disabled: bool,
}

impl PlayerLaunch {
    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub const fn max_memory_cache_bytes(&self) -> u64 {
        self.max_memory_cache_bytes
    }

    pub const fn disk_cache_disabled(&self) -> bool {
        self.disk_cache_disabled
    }
}

#[derive(Clone, Debug)]
pub struct VideoPreviewSession {
    key: PreviewKey,
    upstream: VideoUpstream,
    token: VideoToken,
    endpoint: LoopbackEndpoint,
    budget: VideoCacheBudget,
    active: bool,
}

impl VideoPreviewSession {
    pub fn new(
        key: PreviewKey,
        upstream: VideoUpstream,
        token: VideoToken,
        endpoint: LoopbackEndpoint,
        budget: VideoCacheBudget,
    ) -> Result<Self, VideoPreviewError> {
        Ok(Self {
            key,
            upstream,
            token,
            endpoint,
            budget,
            active: true,
        })
    }

    pub fn player_url(&self) -> String {
        format!(
            "http://{}:{}{}{}",
            self.endpoint.host(),
            self.endpoint.port(),
            VIDEO_PREVIEW_PATH,
            self.token.as_str()
        )
    }

    pub fn upstream(&self) -> &VideoUpstream {
        &self.upstream
    }

    pub const fn is_active(&self) -> bool {
        self.active
    }

    pub fn invalidate(&mut self) {
        self.active = false;
    }

    pub fn handle_proxy_request(
        &self,
        request: VideoProxyRequest,
    ) -> Result<VideoProxyResponse, VideoPreviewError> {
        if !self.active {
            return Err(VideoPreviewError::InactiveVideoSession);
        }
        if !self.token.matches(&request.token) {
            return Err(VideoPreviewError::InvalidVideoToken);
        }

        let _upstream_url = self.upstream.as_str();
        match request.method {
            VideoProxyMethod::Head => Ok(VideoProxyResponse::Head {
                content_length: self.key.size,
                accept_ranges: true,
            }),
            VideoProxyMethod::Get => {
                let range = request
                    .range
                    .unwrap_or_else(|| self.default_range())
                    .clamp_to_file(self.key.size)?;
                Ok(VideoProxyResponse::Fetch(range))
            }
        }
    }

    pub fn player_launch(&self, backend: PlayerBackend) -> Result<PlayerLaunch, VideoPreviewError> {
        match backend {
            PlayerBackend::Mpv => Ok(PlayerLaunch {
                program: "mpv".to_string(),
                args: vec![
                    "--disk-cache=no".to_string(),
                    "--cache=yes".to_string(),
                    format!("--demuxer-max-bytes={}", self.budget.memory_limit_bytes()),
                    "--demuxer-max-back-bytes=0".to_string(),
                    "--no-ytdl".to_string(),
                    self.player_url(),
                ],
                max_memory_cache_bytes: self.budget.memory_limit_bytes(),
                disk_cache_disabled: true,
            }),
        }
    }

    fn default_range(&self) -> VideoRange {
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
