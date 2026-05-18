use std::{
    collections::{HashMap, VecDeque},
    fmt,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
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
    Pdf,
    Archive,
    Directory,
    Metadata,
    Video,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewAction {
    FetchText,
    FetchImageMetadata,
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
            return Self::metadata(PreviewKind::Directory, "directory metadata only");
        }

        match preview_kind_from_name(file_name) {
            PreviewKind::Video => {
                Self::metadata(PreviewKind::Video, "video preview requires explicit action")
            }
            PreviewKind::Image => {
                if size_bytes > memory_limit_bytes {
                    Self::metadata(PreviewKind::Image, "image exceeds preview memory budget")
                } else {
                    Self {
                        kind: PreviewKind::Image,
                        action: PreviewAction::FetchImageMetadata,
                        reason: "image header preview within memory budget".to_string(),
                    }
                }
            }
            PreviewKind::Text => Self {
                kind: if size_bytes > memory_limit_bytes {
                    PreviewKind::Metadata
                } else {
                    PreviewKind::Text
                },
                action: if size_bytes > memory_limit_bytes {
                    PreviewAction::MetadataOnly
                } else {
                    PreviewAction::FetchText
                },
                reason: if size_bytes > memory_limit_bytes {
                    "file exceeds preview memory budget"
                } else {
                    "text preview within memory budget"
                }
                .to_string(),
            },
            PreviewKind::Pdf => Self::metadata(PreviewKind::Pdf, "pdf metadata preview only"),
            PreviewKind::Archive => {
                Self::metadata(PreviewKind::Archive, "archive metadata preview only")
            }
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
        | "xml" | "csv" | "ini" | "conf" | "sh" | "bash" | "zsh" | "fish" | "srt" | "ass"
        | "vtt" => PreviewKind::Text,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "avif" => PreviewKind::Image,
        "pdf" => PreviewKind::Pdf,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" => PreviewKind::Archive,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "flv" => PreviewKind::Video,
        _ => PreviewKind::Metadata,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImagePreviewFormat {
    Png,
    Jpeg,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImagePreviewMetadata {
    pub format: ImagePreviewFormat,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub disk_backed: bool,
}

pub fn parse_image_metadata(bytes: &[u8]) -> ImagePreviewMetadata {
    if let Some((width, height)) = parse_png_dimensions(bytes) {
        return ImagePreviewMetadata {
            format: ImagePreviewFormat::Png,
            width: Some(width),
            height: Some(height),
            disk_backed: false,
        };
    }
    if let Some((width, height)) = parse_jpeg_dimensions(bytes) {
        return ImagePreviewMetadata {
            format: ImagePreviewFormat::Jpeg,
            width: Some(width),
            height: Some(height),
            disk_backed: false,
        };
    }

    ImagePreviewMetadata {
        format: ImagePreviewFormat::Unknown,
        width: None,
        height: None,
        disk_backed: false,
    }
}

fn parse_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

fn parse_jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4
        || bytes.first().copied() != Some(0xff)
        || bytes.get(1).copied() != Some(0xd8)
    {
        return None;
    }

    let mut cursor = 2;
    while cursor + 8 < bytes.len() {
        if bytes.get(cursor).copied() != Some(0xff) {
            cursor += 1;
            continue;
        }
        let marker = bytes.get(cursor + 1).copied()?;
        cursor += 2;
        if marker == 0xd8 || marker == 0xd9 {
            continue;
        }
        let length = u16::from_be_bytes(bytes.get(cursor..cursor + 2)?.try_into().ok()?);
        let length = usize::from(length);
        if length < 2 || cursor + length > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if length < 7 {
                return None;
            }
            let height = u16::from_be_bytes(bytes.get(cursor + 3..cursor + 5)?.try_into().ok()?);
            let width = u16::from_be_bytes(bytes.get(cursor + 5..cursor + 7)?.try_into().ok()?);
            return Some((u32::from(width), u32::from(height)));
        }
        cursor += length;
    }
    None
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

    pub fn request_state(&self, request: PreviewRequest) -> RequestState {
        if self.generations.get(&request.key).copied() == Some(request.generation) {
            RequestState::Applied
        } else {
            RequestState::Stale
        }
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
    LoopbackServerIo,
    MalformedHttpRequest,
    UnsupportedHttpMethod,
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

    fn clamp_len(self, max_len: u64) -> Result<Self, VideoPreviewError> {
        if max_len == 0 {
            return Err(VideoPreviewError::InvalidCacheBudget);
        }

        Ok(Self {
            start: self.start,
            end_inclusive: self
                .end_inclusive
                .min(self.start.saturating_add(max_len).saturating_sub(1)),
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

    pub fn window(&self) -> Option<VideoRange> {
        let mut start = u64::MAX;
        let mut end = 0;
        for (offset, bytes) in &self.chunks {
            start = start.min(*offset);
            let chunk_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            end = end.max(offset.saturating_add(chunk_len).saturating_sub(1));
        }

        if start == u64::MAX {
            None
        } else {
            Some(VideoRange {
                start,
                end_inclusive: end,
            })
        }
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VideoProxyStats {
    pub used_bytes: u64,
    pub cache_hits: u64,
    pub upstream_fetches: u64,
    pub last_range_start: Option<u64>,
    pub last_range_end_inclusive: Option<u64>,
    pub window_start: Option<u64>,
    pub window_end_inclusive: Option<u64>,
    pub last_error: Option<VideoPreviewError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerBackend {
    Mpv,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MpvLaunchProfile {
    User,
    Isolated,
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
                    .clamp_to_file(self.key.size)?
                    .clamp_len(self.budget.chunk_size_bytes)?;
                Ok(VideoProxyResponse::Fetch(range))
            }
        }
    }

    pub fn player_launch(&self, backend: PlayerBackend) -> Result<PlayerLaunch, VideoPreviewError> {
        self.player_launch_with_profile(backend, MpvLaunchProfile::User)
    }

    pub fn player_launch_with_profile(
        &self,
        backend: PlayerBackend,
        profile: MpvLaunchProfile,
    ) -> Result<PlayerLaunch, VideoPreviewError> {
        match backend {
            PlayerBackend::Mpv => {
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
                    "--force-window=yes".to_string(),
                ]);
                if profile == MpvLaunchProfile::Isolated {
                    args.extend(["--ao=null".to_string(), "--mute=yes".to_string()]);
                }
                args.extend([
                    "--idle=yes".to_string(),
                    "--really-quiet".to_string(),
                    "--no-ytdl".to_string(),
                    self.player_url(),
                ]);
                Ok(PlayerLaunch {
                    program: "mpv".to_string(),
                    args,
                    max_memory_cache_bytes: self.budget.memory_limit_bytes(),
                    disk_cache_disabled: true,
                })
            }
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

type VideoRangeFetcher =
    dyn Fn(VideoRange) -> Result<Vec<u8>, VideoPreviewError> + Send + Sync + 'static;

struct VideoLoopbackState {
    session: VideoPreviewSession,
    cache: VideoChunkCache,
    stats: VideoProxyStats,
    fetcher: Arc<VideoRangeFetcher>,
}

pub struct VideoLoopbackServer {
    endpoint: LoopbackEndpoint,
    player_url: String,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<VideoLoopbackState>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl VideoLoopbackServer {
    pub fn spawn<F>(
        key: PreviewKey,
        upstream: VideoUpstream,
        token: VideoToken,
        budget: VideoCacheBudget,
        fetcher: F,
    ) -> Result<Self, VideoPreviewError>
    where
        F: Fn(VideoRange) -> Result<Vec<u8>, VideoPreviewError> + Send + Sync + 'static,
    {
        let listener = TcpListener::bind((LOOPBACK_HOST, 0))
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        let port = listener
            .local_addr()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?
            .port();
        let endpoint = LoopbackEndpoint::new(port)?;
        let session = VideoPreviewSession::new(key, upstream, token, endpoint.clone(), budget)?;
        let player_url = session.player_url();
        let state = Arc::new(Mutex::new(VideoLoopbackState {
            session,
            cache: VideoChunkCache::new(budget),
            stats: VideoProxyStats::default(),
            fetcher: Arc::new(fetcher),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let handle =
            thread::spawn(move || serve_video_loopback(listener, thread_state, thread_stop));

        Ok(Self {
            endpoint,
            player_url,
            stop,
            state,
            handle: Some(handle),
        })
    }

    pub const fn endpoint(&self) -> &LoopbackEndpoint {
        &self.endpoint
    }

    pub fn player_url(&self) -> &str {
        &self.player_url
    }

    pub fn stats(&self) -> VideoProxyStats {
        match self.state.lock() {
            Ok(state) => {
                let mut stats = state.stats.clone();
                stats.used_bytes = state.cache.used_bytes();
                if let Some(window) = state.cache.window() {
                    stats.window_start = Some(window.start);
                    stats.window_end_inclusive = Some(window.end_inclusive);
                }
                stats
            }
            Err(_) => VideoProxyStats::default(),
        }
    }

    pub fn player_launch(&self, backend: PlayerBackend) -> Result<PlayerLaunch, VideoPreviewError> {
        self.state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?
            .session
            .player_launch(backend)
    }

    pub fn player_launch_with_profile(
        &self,
        backend: PlayerBackend,
        profile: MpvLaunchProfile,
    ) -> Result<PlayerLaunch, VideoPreviewError> {
        self.state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?
            .session
            .player_launch_with_profile(backend, profile)
    }

    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(stream) = TcpStream::connect((LOOPBACK_HOST, self.endpoint.port())) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for VideoLoopbackServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve_video_loopback(
    listener: TcpListener,
    state: Arc<Mutex<VideoLoopbackState>>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                handle_video_connection(stream, &state);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn handle_video_connection(mut stream: TcpStream, state: &Arc<Mutex<VideoLoopbackState>>) {
    let response = match read_http_request(&mut stream)
        .and_then(|request| handle_http_request(request, state))
    {
        Ok(response) => response,
        Err(err) => video_error_response(err),
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
}

struct ParsedHttpRequest {
    method: VideoProxyMethod,
    token: String,
    range: Option<VideoRange>,
}

fn read_http_request(stream: &mut TcpStream) -> Result<ParsedHttpRequest, VideoPreviewError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
    let mut data = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..read]);
        if data.windows(4).any(|window| window == b"\r\n\r\n") || data.len() > 16 * 1024 {
            break;
        }
    }

    parse_http_request(&String::from_utf8_lossy(&data))
}

fn parse_http_request(request: &str) -> Result<ParsedHttpRequest, VideoPreviewError> {
    let mut lines = request.split("\r\n");
    let line = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or(VideoPreviewError::MalformedHttpRequest)?;
    let mut parts = line.split_whitespace();
    let method = match parts.next() {
        Some("HEAD") => VideoProxyMethod::Head,
        Some("GET") => VideoProxyMethod::Get,
        Some(_) => return Err(VideoPreviewError::UnsupportedHttpMethod),
        None => return Err(VideoPreviewError::MalformedHttpRequest),
    };
    let path = parts
        .next()
        .ok_or(VideoPreviewError::MalformedHttpRequest)?;
    let token = path
        .strip_prefix(VIDEO_PREVIEW_PATH)
        .and_then(|value| value.split(['?', '/']).next())
        .filter(|value| !value.is_empty())
        .ok_or(VideoPreviewError::InvalidVideoToken)?
        .to_string();
    let range = lines.find_map(parse_range_header).transpose()?;

    Ok(ParsedHttpRequest {
        method,
        token,
        range,
    })
}

fn parse_range_header(line: &str) -> Option<Result<VideoRange, VideoPreviewError>> {
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("range") {
        return None;
    }

    let value = value.trim();
    let range = value.strip_prefix("bytes=")?;
    let (start, end) = range.split_once('-')?;
    let Ok(start) = start.parse::<u64>() else {
        return Some(Err(VideoPreviewError::InvalidRange));
    };
    let end = if end.is_empty() {
        u64::MAX
    } else {
        match end.parse::<u64>() {
            Ok(end) => end,
            Err(_) => return Some(Err(VideoPreviewError::InvalidRange)),
        }
    };

    Some(VideoRange::new(start, end))
}

fn handle_http_request(
    request: ParsedHttpRequest,
    state: &Arc<Mutex<VideoLoopbackState>>,
) -> Result<Vec<u8>, VideoPreviewError> {
    let decision = {
        let state = state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        state.session.handle_proxy_request(VideoProxyRequest {
            method: request.method,
            token: request.token,
            range: request.range,
        })?
    };

    match decision {
        VideoProxyResponse::Head {
            content_length,
            accept_ranges,
        } => Ok(video_head_response(content_length, accept_ranges)),
        VideoProxyResponse::Fetch(range) => video_get_response(range, state),
    }
}

fn video_get_response(
    range: VideoRange,
    state: &Arc<Mutex<VideoLoopbackState>>,
) -> Result<Vec<u8>, VideoPreviewError> {
    {
        let mut state = state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        state.stats.last_range_start = Some(range.start);
        state.stats.last_range_end_inclusive = Some(range.end_inclusive);
    }

    let cached = {
        let mut state = state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        if let Some(bytes) = state.cache.get(range.start).map(<[u8]>::to_vec) {
            state.stats.cache_hits = state.stats.cache_hits.saturating_add(1);
            state.stats.last_error = None;
            Some((bytes, state.session.key.size))
        } else {
            None
        }
    };
    let (bytes, file_size) = match cached {
        Some(hit) => hit,
        None => {
            let fetcher = {
                let state = state
                    .lock()
                    .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
                Arc::clone(&state.fetcher)
            };
            let bytes = match fetcher(range) {
                Ok(bytes) => bytes,
                Err(err) => {
                    if let Ok(mut state) = state.lock() {
                        state.stats.last_error = Some(err.clone());
                    }
                    return Err(err);
                }
            };
            let file_size = {
                let mut state = state
                    .lock()
                    .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
                state.cache.put(range.start, bytes.clone())?;
                state.stats.upstream_fetches = state.stats.upstream_fetches.saturating_add(1);
                state.stats.last_error = None;
                state.session.key.size
            };
            (bytes, file_size)
        }
    };
    let body_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let response_end = range.start.saturating_add(body_len).saturating_sub(1);

    Ok(video_body_response(
        "206 Partial Content",
        &[
            ("Accept-Ranges", "bytes".to_string()),
            (
                "Content-Range",
                format!("bytes {}-{}/{}", range.start, response_end, file_size),
            ),
            ("Content-Length", bytes.len().to_string()),
        ],
        &bytes,
    ))
}

fn video_head_response(content_length: u64, accept_ranges: bool) -> Vec<u8> {
    let accept_ranges = if accept_ranges { "bytes" } else { "none" };
    video_body_response(
        "200 OK",
        &[
            ("Accept-Ranges", accept_ranges.to_string()),
            ("Content-Length", content_length.to_string()),
        ],
        &[],
    )
}

fn video_error_response(err: VideoPreviewError) -> Vec<u8> {
    let status = match err {
        VideoPreviewError::InvalidVideoToken => "403 Forbidden",
        VideoPreviewError::InactiveVideoSession => "410 Gone",
        VideoPreviewError::InvalidRange => "416 Range Not Satisfiable",
        VideoPreviewError::UnsupportedHttpMethod => "405 Method Not Allowed",
        VideoPreviewError::MalformedHttpRequest => "400 Bad Request",
        VideoPreviewError::InvalidUpstreamUrl
        | VideoPreviewError::InvalidVideoTokenFormat
        | VideoPreviewError::InvalidCacheBudget
        | VideoPreviewError::InvalidPort
        | VideoPreviewError::NonLoopbackBindAddress
        | VideoPreviewError::VideoChunkTooLarge
        | VideoPreviewError::LoopbackServerIo => "500 Internal Server Error",
    };
    video_body_response(status, &[("Content-Length", "0".to_string())], &[])
}

fn video_body_response(status: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");

    let mut response = head.into_bytes();
    response.extend_from_slice(body);
    response
}
