use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::Duration,
};

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
    Audio,
    Document,
    Ebook,
    Font,
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
            PreviewKind::Audio => Self::metadata(PreviewKind::Audio, "audio metadata preview only"),
            PreviewKind::Document => {
                Self::metadata(PreviewKind::Document, "document metadata preview only")
            }
            PreviewKind::Ebook => Self::metadata(PreviewKind::Ebook, "ebook metadata preview only"),
            PreviewKind::Font => Self::metadata(PreviewKind::Font, "font metadata preview only"),
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
}

pub fn preview_kind_from_name(file_name: &str) -> PreviewKind {
    let lower_name = file_name.to_ascii_lowercase();
    if matches!(
        lower_name.as_str(),
        "sha256sums" | "sha256sum" | "sha512sums" | "md5sums" | "checksums"
    ) {
        return PreviewKind::Text;
    }

    let Some(extension) = file_name.rsplit_once('.').map(|(_, extension)| extension) else {
        return PreviewKind::Metadata;
    };

    match extension.to_ascii_lowercase().as_str() {
        "txt" | "md" | "markdown" | "rs" | "toml" | "json" | "yaml" | "yml" | "log" | "c"
        | "cc" | "cpp" | "h" | "hpp" | "py" | "js" | "ts" | "tsx" | "jsx" | "html" | "css"
        | "xml" | "csv" | "ini" | "conf" | "sh" | "bash" | "zsh" | "fish" | "srt" | "ass"
        | "vtt" | "torrent" | "m3u" | "m3u8" | "pls" | "cue" | "sfv" | "sha256" | "sha512"
        | "md5" => PreviewKind::Text,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "avif" => PreviewKind::Image,
        "pdf" => PreviewKind::Pdf,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" => PreviewKind::Archive,
        "mp3" | "flac" | "wav" | "m4a" | "aac" | "ogg" | "opus" | "ape" | "wma" | "alac" => {
            PreviewKind::Audio
        }
        "doc" | "docx" | "ppt" | "pptx" | "xls" | "xlsx" | "odt" | "ods" | "odp" | "rtf" => {
            PreviewKind::Document
        }
        "epub" | "mobi" | "azw3" | "fb2" => PreviewKind::Ebook,
        "ttf" | "otf" | "woff" | "woff2" => PreviewKind::Font,
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
}

pub fn parse_image_metadata(bytes: &[u8]) -> ImagePreviewMetadata {
    if let Some((width, height)) = parse_png_dimensions(bytes) {
        return ImagePreviewMetadata {
            format: ImagePreviewFormat::Png,
            width: Some(width),
            height: Some(height),
        };
    }
    if let Some((width, height)) = parse_jpeg_dimensions(bytes) {
        return ImagePreviewMetadata {
            format: ImagePreviewFormat::Jpeg,
            width: Some(width),
            height: Some(height),
        };
    }

    ImagePreviewMetadata {
        format: ImagePreviewFormat::Unknown,
        width: None,
        height: None,
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
pub struct PreviewRequest {
    key: PreviewKey,
    generation: u64,
}

impl PreviewRequest {
    pub const fn key(self) -> PreviewKey {
        self.key
    }
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

    pub fn is_current_request(&self, request: PreviewRequest) -> bool {
        self.generations.get(&request.key).copied() == Some(request.generation)
    }

    pub fn finish_text_request(
        &mut self,
        request: PreviewRequest,
        bytes: &[u8],
    ) -> Option<PreviewStatus> {
        if !self.is_current_request(request) {
            return None;
        }

        let status = self.text_status(bytes);
        let content_len = usize::try_from(status.fetched_bytes)
            .unwrap_or(bytes.len())
            .min(bytes.len());
        let content = String::from_utf8_lossy(&bytes[..content_len]).to_string();
        self.insert(request.key, PreviewText { content, status });

        Some(status)
    }

    pub fn get(&self, key: &PreviewKey) -> Option<&PreviewText> {
        self.cache.get(key)
    }

    fn text_status(&self, bytes: &[u8]) -> PreviewStatus {
        let incoming = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let fetched = incoming.min(self.memory_limit_bytes);
        PreviewStatus {
            kind: PreviewKind::Text,
            fetched_bytes: fetched,
            memory_limit_bytes: self.memory_limit_bytes,
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
    InvalidVideoTokenFormat,
    InvalidCacheBudget,
    InvalidRange,
    InvalidVideoToken,
    InactiveVideoSession,
    VideoChunkTooLarge,
    LoopbackServerIo,
    MalformedHttpRequest,
    UnsupportedHttpMethod,
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

    fn as_str(&self) -> &str {
        &self.token
    }

    fn matches(&self, candidate: &str) -> bool {
        self.token == candidate
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoCacheBudget {
    chunk_size_bytes: u64,
    max_chunks: usize,
}

impl VideoCacheBudget {
    pub const fn new(chunk_size_bytes: u64, max_chunks: usize) -> Result<Self, VideoPreviewError> {
        if chunk_size_bytes == 0 || max_chunks == 0 {
            return Err(VideoPreviewError::InvalidCacheBudget);
        }

        Ok(Self {
            chunk_size_bytes,
            max_chunks,
        })
    }

    fn memory_limit_bytes(&self) -> u64 {
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
    pub const fn new(start: u64, end_inclusive: u64) -> Result<Self, VideoPreviewError> {
        if start > end_inclusive {
            return Err(VideoPreviewError::InvalidRange);
        }
        Ok(Self {
            start,
            end_inclusive,
        })
    }

    const fn len(&self) -> u64 {
        if self.is_empty() {
            0
        } else {
            self.end_inclusive
                .saturating_sub(self.start)
                .saturating_add(1)
        }
    }

    const fn is_empty(&self) -> bool {
        self.start > self.end_inclusive
    }

    const fn contains(&self, other: Self) -> bool {
        self.start <= other.start && self.end_inclusive >= other.end_inclusive
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestedVideoRange {
    Explicit(VideoRange),
    OpenEnded(u64),
    Suffix(u64),
}

impl RequestedVideoRange {
    const fn suffix(bytes: u64) -> Result<Self, VideoPreviewError> {
        if bytes == 0 {
            return Err(VideoPreviewError::InvalidRange);
        }
        Ok(Self::Suffix(bytes))
    }

    fn resolve(self, file_size: u64, max_len: u64) -> Result<VideoRange, VideoPreviewError> {
        match self {
            Self::Explicit(range) => range.clamp_to_file(file_size)?.clamp_len(max_len),
            Self::OpenEnded(start) => {
                if file_size == 0 || start >= file_size || max_len == 0 {
                    return Err(VideoPreviewError::InvalidRange);
                }
                VideoRange::new(
                    start,
                    start
                        .saturating_add(max_len)
                        .saturating_sub(1)
                        .min(file_size.saturating_sub(1)),
                )
            }
            Self::Suffix(bytes) => {
                if file_size == 0 || max_len == 0 || bytes == 0 {
                    return Err(VideoPreviewError::InvalidRange);
                }
                let len = bytes.min(file_size).min(max_len);
                VideoRange::new(file_size.saturating_sub(len), file_size.saturating_sub(1))
            }
        }
    }
}

impl From<VideoRange> for RequestedVideoRange {
    fn from(range: VideoRange) -> Self {
        Self::Explicit(range)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedInterval {
    range: VideoRange,
    bytes: Vec<u8>,
    last_used: u64,
}

#[derive(Clone, Debug)]
struct VideoIntervalBuffer {
    budget: VideoCacheBudget,
    used_bytes: u64,
    tick: u64,
    intervals: Vec<CachedInterval>,
}

impl VideoIntervalBuffer {
    const fn new(budget: VideoCacheBudget) -> Self {
        Self {
            budget,
            used_bytes: 0,
            tick: 0,
            intervals: Vec::new(),
        }
    }

    fn put(&mut self, range: VideoRange, bytes: Vec<u8>) -> Result<(), VideoPreviewError> {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if len > range.len() || len > self.budget.memory_limit_bytes() {
            return Err(VideoPreviewError::VideoChunkTooLarge);
        }

        self.remove_exact(range);
        let last_used = self.next_tick();
        self.used_bytes = self.used_bytes.saturating_add(len);
        self.intervals.push(CachedInterval {
            range,
            bytes,
            last_used,
        });
        self.evict_until_within_budget();
        Ok(())
    }

    fn get(&mut self, range: VideoRange) -> Option<Vec<u8>> {
        let index = self
            .intervals
            .iter()
            .position(|interval| interval.range.contains(range))?;
        let offset = usize::try_from(
            range
                .start
                .saturating_sub(self.intervals[index].range.start),
        )
        .ok()?;
        let len = usize::try_from(range.len()).ok()?;
        let end = offset.checked_add(len)?;
        let bytes = self.intervals[index].bytes.get(offset..end)?.to_vec();
        self.intervals[index].last_used = self.next_tick();
        Some(bytes)
    }

    fn window(&self) -> Option<VideoRange> {
        let mut start = u64::MAX;
        let mut end = 0;
        for interval in &self.intervals {
            start = start.min(interval.range.start);
            end = end.max(interval.range.end_inclusive);
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

    fn contains_range(&self, range: VideoRange) -> bool {
        self.intervals
            .iter()
            .any(|interval| interval.range.contains(range))
    }

    fn remove_exact(&mut self, range: VideoRange) {
        let mut index = 0;
        while index < self.intervals.len() {
            if self.intervals[index].range == range {
                let old = self.intervals.remove(index);
                let len = u64::try_from(old.bytes.len()).unwrap_or(u64::MAX);
                self.used_bytes = self.used_bytes.saturating_sub(len);
            } else {
                index += 1;
            }
        }
    }

    fn evict_until_within_budget(&mut self) {
        while self.used_bytes > self.budget.memory_limit_bytes() {
            let Some(index) = self
                .intervals
                .iter()
                .enumerate()
                .min_by_key(|(_index, interval)| interval.last_used)
                .map(|(index, _interval)| index)
            else {
                break;
            };
            let old = self.intervals.remove(index);
            let len = u64::try_from(old.bytes.len()).unwrap_or(u64::MAX);
            self.used_bytes = self.used_bytes.saturating_sub(len);
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VideoProxyMethod {
    Head,
    Get,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VideoProxyRequest {
    method: VideoProxyMethod,
    token: String,
    range: Option<RequestedVideoRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VideoProxyResponse {
    Head {
        content_length: u64,
        accept_ranges: bool,
    },
    Fetch {
        range: VideoRange,
        force_prefetch: bool,
    },
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
    pub last_error: Option<VideoPreviewError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerLogRing {
    limit: usize,
    lines: VecDeque<String>,
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
    fn lines(&self) -> Vec<&str> {
        self.lines.iter().map(String::as_str).collect()
    }

    pub fn last_line(&self) -> Option<&str> {
        self.lines.back().map(String::as_str)
    }
}

fn sanitize_player_log_line(line: &str) -> String {
    line.split_whitespace()
        .map(sanitize_player_log_part)
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_player_log_part(part: &str) -> String {
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

fn is_sensitive_log_key(key: &str) -> bool {
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
struct VideoPreviewSession {
    key: PreviewKey,
    token: VideoToken,
    port: u16,
    budget: VideoCacheBudget,
}

impl VideoPreviewSession {
    const fn new(key: PreviewKey, token: VideoToken, port: u16, budget: VideoCacheBudget) -> Self {
        Self {
            key,
            token,
            port,
            budget,
        }
    }

    fn player_url(&self) -> String {
        format!(
            "http://{}:{}{}{}",
            LOOPBACK_HOST,
            self.port,
            VIDEO_PREVIEW_PATH,
            self.token.as_str()
        )
    }

    fn handle_proxy_request(
        &self,
        request: &VideoProxyRequest,
    ) -> Result<VideoProxyResponse, VideoPreviewError> {
        if !self.token.matches(&request.token) {
            return Err(VideoPreviewError::InvalidVideoToken);
        }

        match request.method {
            VideoProxyMethod::Head => Ok(VideoProxyResponse::Head {
                content_length: self.key.size,
                accept_ranges: true,
            }),
            VideoProxyMethod::Get => {
                let requested = request.range;
                let max_len = match requested {
                    Some(RequestedVideoRange::Explicit(_)) => self.budget.memory_limit_bytes(),
                    Some(RequestedVideoRange::OpenEnded(_))
                    | Some(RequestedVideoRange::Suffix(_))
                    | None => self.budget.chunk_size_bytes,
                };
                let force_prefetch =
                    matches!(requested, Some(RequestedVideoRange::OpenEnded(_)) | None);
                let range = requested
                    .unwrap_or_else(|| self.default_range().into())
                    .resolve(self.key.size, max_len)?;
                Ok(VideoProxyResponse::Fetch {
                    range,
                    force_prefetch,
                })
            }
        }
    }

    fn mpv_args_with_profile(&self, profile: MpvLaunchProfile) -> Vec<String> {
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
    cache: VideoIntervalBuffer,
    inflight_prefetches: Vec<VideoRange>,
    prefetch_chunks: usize,
    stats: VideoProxyStats,
    fetcher: Arc<VideoRangeFetcher>,
    stop: Arc<AtomicBool>,
}

#[derive(Clone)]
struct VideoPrefetchJob {
    range: VideoRange,
    prefetch_chunks: usize,
    chunk_size_bytes: u64,
    memory_limit_bytes: u64,
    file_size: u64,
    fetcher: Arc<VideoRangeFetcher>,
    state: Arc<Mutex<VideoLoopbackState>>,
    ready: Arc<Condvar>,
    stop: Arc<AtomicBool>,
}

pub struct VideoLoopbackServer {
    port: u16,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<VideoLoopbackState>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl VideoLoopbackServer {
    pub fn spawn_with_prefetch<F>(
        key: PreviewKey,
        token: VideoToken,
        budget: VideoCacheBudget,
        prefetch_chunks: usize,
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
        let session = VideoPreviewSession::new(key, token, port, budget);
        let stop = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(Condvar::new());
        let state = Arc::new(Mutex::new(VideoLoopbackState {
            session,
            cache: VideoIntervalBuffer::new(budget),
            inflight_prefetches: Vec::new(),
            prefetch_chunks,
            stats: VideoProxyStats::default(),
            fetcher: Arc::new(fetcher),
            stop: Arc::clone(&stop),
        }));
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let thread_ready = Arc::clone(&ready);
        let handle = thread::spawn(move || {
            serve_video_loopback(&listener, &thread_state, &thread_ready, &thread_stop);
        });

        Ok(Self {
            port,
            stop,
            state,
            handle: Some(handle),
        })
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn stats(&self) -> VideoProxyStats {
        match self.state.lock() {
            Ok(state) => {
                let mut stats = state.stats.clone();
                stats.used_bytes = state.cache.used_bytes;
                if let Some(window) = state.cache.window() {
                    stats.window_start = Some(window.start);
                    stats.window_end_inclusive = Some(window.end_inclusive);
                }
                stats
            }
            Err(_) => VideoProxyStats::default(),
        }
    }

    pub fn mpv_args_with_profile(
        &self,
        profile: MpvLaunchProfile,
    ) -> Result<Vec<String>, VideoPreviewError> {
        let state = self
            .state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        Ok(state.session.mpv_args_with_profile(profile))
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(stream) = TcpStream::connect((LOOPBACK_HOST, self.port)) {
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
    listener: &TcpListener,
    state: &Arc<Mutex<VideoLoopbackState>>,
    ready: &Arc<Condvar>,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let state = Arc::clone(state);
                let ready = Arc::clone(ready);
                thread::spawn(move || handle_video_connection(stream, &state, &ready));
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn handle_video_connection(
    mut stream: TcpStream,
    state: &Arc<Mutex<VideoLoopbackState>>,
    ready: &Arc<Condvar>,
) {
    let response = match read_http_request(&mut stream)
        .and_then(|request| handle_http_request(request, state, ready))
    {
        Ok(response) => response,
        Err(err) => video_error_response(&err),
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
}

struct ParsedHttpRequest {
    method: VideoProxyMethod,
    token: String,
    range: Option<RequestedVideoRange>,
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
    let range = lines.find_map(parse_video_range_header).transpose()?;

    Ok(ParsedHttpRequest {
        method,
        token,
        range,
    })
}

fn parse_video_range_header(line: &str) -> Option<Result<RequestedVideoRange, VideoPreviewError>> {
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("range") {
        return None;
    }

    let value = value.trim();
    let range = value.strip_prefix("bytes=")?;
    let (start, end) = range.split_once('-')?;
    if start.is_empty() {
        return Some(
            end.parse::<u64>()
                .map_err(|_| VideoPreviewError::InvalidRange)
                .and_then(RequestedVideoRange::suffix),
        );
    }
    let Ok(start) = start.parse::<u64>() else {
        return Some(Err(VideoPreviewError::InvalidRange));
    };
    let end = if end.is_empty() {
        return Some(Ok(RequestedVideoRange::OpenEnded(start)));
    } else {
        match end.parse::<u64>() {
            Ok(end) => end,
            Err(_) => return Some(Err(VideoPreviewError::InvalidRange)),
        }
    };

    Some(VideoRange::new(start, end).map(Into::into))
}

fn handle_http_request(
    request: ParsedHttpRequest,
    state: &Arc<Mutex<VideoLoopbackState>>,
    ready: &Arc<Condvar>,
) -> Result<Vec<u8>, VideoPreviewError> {
    let decision = {
        let state = state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        state.session.handle_proxy_request(&VideoProxyRequest {
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
        VideoProxyResponse::Fetch {
            range,
            force_prefetch,
        } => video_get_response(range, force_prefetch, state, ready),
    }
}

fn video_get_response(
    range: VideoRange,
    force_prefetch: bool,
    state: &Arc<Mutex<VideoLoopbackState>>,
    ready: &Arc<Condvar>,
) -> Result<Vec<u8>, VideoPreviewError> {
    {
        let mut state = state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        state.stats.last_range_start = Some(range.start);
        state.stats.last_range_end_inclusive = Some(range.end_inclusive);
    }

    let (cached, window_before) = {
        let mut state = state
            .lock()
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        if let Some(bytes) = state.cache.get(range) {
            state.stats.cache_hits = state.stats.cache_hits.saturating_add(1);
            state.stats.last_error = None;
            (Some((bytes, state.session.key.size)), None)
        } else {
            (None, state.cache.window())
        }
    };
    let (bytes, file_size) = match cached {
        Some(hit) => hit,
        None => {
            if let Some(hit) = wait_for_inflight_prefetch(range, state, ready)? {
                hit
            } else {
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
                    if let Err(cache_err) = state.cache.put(range, bytes.clone()) {
                        state.stats.last_error = Some(cache_err);
                    } else {
                        state.stats.last_error = None;
                    }
                    state.stats.upstream_fetches = state.stats.upstream_fetches.saturating_add(1);
                    state.session.key.size
                };
                spawn_prefetch_after(range, window_before, force_prefetch, state, ready);
                (bytes, file_size)
            }
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

fn wait_for_inflight_prefetch(
    range: VideoRange,
    state: &Arc<Mutex<VideoLoopbackState>>,
    ready: &Condvar,
) -> Result<Option<(Vec<u8>, u64)>, VideoPreviewError> {
    const MAX_PREFETCH_WAIT_LOOPS: usize = 60;
    let mut state_guard = state
        .lock()
        .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
    let mut loops_remaining = MAX_PREFETCH_WAIT_LOOPS;
    while inflight_contains_range(&state_guard.inflight_prefetches, range) && loops_remaining > 0 {
        let wait_result = ready
            .wait_timeout(state_guard, Duration::from_millis(250))
            .map_err(|_| VideoPreviewError::LoopbackServerIo)?;
        state_guard = wait_result.0;
        loops_remaining -= 1;
    }
    if let Some(bytes) = state_guard.cache.get(range) {
        state_guard.stats.cache_hits = state_guard.stats.cache_hits.saturating_add(1);
        state_guard.stats.last_error = None;
        return Ok(Some((bytes, state_guard.session.key.size)));
    }
    Ok(None)
}

fn spawn_prefetch_after(
    range: VideoRange,
    window_before: Option<VideoRange>,
    force_prefetch: bool,
    state: &Arc<Mutex<VideoLoopbackState>>,
    ready: &Arc<Condvar>,
) {
    let job = {
        let Ok(state_guard) = state.lock() else {
            return;
        };
        let prefetch_chunks = state_guard.prefetch_chunks;
        if prefetch_chunks == 0 {
            return;
        }
        if !should_prefetch_after(
            range,
            window_before,
            force_prefetch,
            state_guard.session.budget.chunk_size_bytes,
            state_guard.session.budget.memory_limit_bytes(),
        ) {
            return;
        }
        VideoPrefetchJob {
            range,
            prefetch_chunks,
            chunk_size_bytes: state_guard.session.budget.chunk_size_bytes,
            memory_limit_bytes: state_guard.session.budget.memory_limit_bytes(),
            file_size: state_guard.session.key.size,
            fetcher: Arc::clone(&state_guard.fetcher),
            state: Arc::clone(state),
            ready: Arc::clone(ready),
            stop: Arc::clone(&state_guard.stop),
        }
    };

    thread::spawn(move || {
        prefetch_forward_window(&job);
    });
}

fn should_prefetch_after(
    range: VideoRange,
    window_before: Option<VideoRange>,
    force_prefetch: bool,
    chunk_size_bytes: u64,
    memory_limit_bytes: u64,
) -> bool {
    if range.len() >= memory_limit_bytes {
        return false;
    }
    if force_prefetch {
        return true;
    }
    match window_before {
        None => range.start == 0 || range.len() == chunk_size_bytes,
        Some(window) => {
            range.start
                <= window
                    .end_inclusive
                    .saturating_add(chunk_size_bytes)
                    .saturating_add(1)
        }
    }
}

fn prefetch_forward_window(job: &VideoPrefetchJob) {
    if job.file_size == 0 || job.chunk_size_bytes == 0 || job.memory_limit_bytes == 0 {
        return;
    }
    let Some(mut start) = job.range.end_inclusive.checked_add(1) else {
        return;
    };
    for _ in 0..job.prefetch_chunks {
        if job.stop.load(Ordering::Acquire) || start >= job.file_size {
            break;
        }
        let Some(end) = start.checked_add(job.chunk_size_bytes.saturating_sub(1)) else {
            break;
        };
        let prefetch_range = VideoRange {
            start,
            end_inclusive: end.min(job.file_size.saturating_sub(1)),
        };
        let should_fetch = match job.state.lock() {
            Ok(mut state) => {
                if state.cache.contains_range(prefetch_range)
                    || inflight_contains_range(&state.inflight_prefetches, prefetch_range)
                {
                    false
                } else {
                    state.inflight_prefetches.push(prefetch_range);
                    true
                }
            }
            Err(_) => break,
        };
        if should_fetch {
            let job = job.clone();
            thread::spawn(move || fetch_prefetch_range(job, prefetch_range));
        }
        let Some(next_start) = prefetch_range.end_inclusive.checked_add(1) else {
            break;
        };
        start = next_start;
    }
}

fn fetch_prefetch_range(job: VideoPrefetchJob, range: VideoRange) {
    if job.stop.load(Ordering::Acquire) {
        clear_prefetch_result(&job, range);
        return;
    }

    match (job.fetcher)(range) {
        Ok(bytes) => store_prefetch_result(&job, range, bytes),
        Err(err) => store_prefetch_error(&job, range, err),
    }
}

fn store_prefetch_result(job: &VideoPrefetchJob, range: VideoRange, bytes: Vec<u8>) {
    if let Ok(mut state) = job.state.lock() {
        clear_inflight_prefetch(&mut state, range);
        match state.cache.put(range, bytes) {
            Ok(()) => {
                state.stats.upstream_fetches = state.stats.upstream_fetches.saturating_add(1);
                state.stats.prefetch_fetches = state.stats.prefetch_fetches.saturating_add(1);
                state.stats.last_error = None;
            }
            Err(err) => {
                state.stats.last_error = Some(err);
            }
        }
        job.ready.notify_all();
    }
}

fn store_prefetch_error(job: &VideoPrefetchJob, range: VideoRange, err: VideoPreviewError) {
    if let Ok(mut state) = job.state.lock() {
        clear_inflight_prefetch(&mut state, range);
        state.stats.last_error = Some(err);
        job.ready.notify_all();
    }
}

fn clear_prefetch_result(job: &VideoPrefetchJob, range: VideoRange) {
    if let Ok(mut state) = job.state.lock() {
        clear_inflight_prefetch(&mut state, range);
        job.ready.notify_all();
    }
}

fn inflight_contains_range(inflight: &[VideoRange], range: VideoRange) -> bool {
    inflight.iter().any(|candidate| candidate.contains(range))
}

fn clear_inflight_prefetch(state: &mut VideoLoopbackState, range: VideoRange) {
    if let Some(index) = state
        .inflight_prefetches
        .iter()
        .position(|inflight| *inflight == range)
    {
        state.inflight_prefetches.remove(index);
    }
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

fn video_error_response(err: &VideoPreviewError) -> Vec<u8> {
    let status = match err {
        VideoPreviewError::InvalidVideoToken => "403 Forbidden",
        VideoPreviewError::InactiveVideoSession => "410 Gone",
        VideoPreviewError::InvalidRange => "416 Range Not Satisfiable",
        VideoPreviewError::UnsupportedHttpMethod => "405 Method Not Allowed",
        VideoPreviewError::MalformedHttpRequest => "400 Bad Request",
        VideoPreviewError::InvalidVideoTokenFormat
        | VideoPreviewError::InvalidCacheBudget
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

#[cfg(test)]
mod tests {
    use super::{
        MpvLaunchProfile, PreviewKey, RequestedVideoRange, VideoCacheBudget, VideoIntervalBuffer,
        VideoPreviewError, VideoPreviewSession, VideoProxyMethod, VideoProxyRequest,
        VideoProxyResponse, VideoRange, VideoToken,
    };

    fn key() -> PreviewKey {
        PreviewKey {
            account_uid: 100,
            fs_id: 42,
            mtime: 7,
            size: 1_000,
        }
    }

    fn session() -> Result<VideoPreviewSession, VideoPreviewError> {
        Ok(VideoPreviewSession::new(
            key(),
            VideoToken::new("tok_0123456789abcdef")?,
            39_177,
            VideoCacheBudget::new(256, 2)?,
        ))
    }

    #[test]
    fn video_session_exposes_only_loopback_token_url_to_player() -> Result<(), VideoPreviewError> {
        let session = session()?;

        assert_eq!(
            session.player_url(),
            "http://127.0.0.1:39177/preview/video/tok_0123456789abcdef"
        );
        assert!(!session.player_url().contains("baidu"));

        let args = session
            .mpv_args_with_profile(MpvLaunchProfile::User)
            .join(" ");
        assert!(args.contains("127.0.0.1:39177"));
        assert!(!args.contains("baidu"));
        assert!(args.contains("--cache-on-disk=no"));
        assert!(!args.contains("--disk-cache=no"));
        assert!(!args.contains("--no-config"));
        assert!(!args.contains("--load-scripts=no"));
        assert!(!args.contains("--osc=no"));
        assert!(args.contains("--demuxer-readahead-secs=20"));
        assert!(args.contains("--cache-pause=no"));
        assert!(args.contains("--no-terminal"));
        assert!(args.contains("--force-window=immediate"));
        assert!(args.contains("--msg-level=all=warn"));
        assert!(!args.contains("--really-quiet"));
        assert!(!args.contains("--vo=null"));
        assert!(!args.contains("--ao=null"));
        assert!(!args.contains("--mute=yes"));
        assert!(!args.contains("--pause"));
        assert!(args.contains("--idle=yes"));
        assert!(args.contains("--demuxer-max-bytes=512"));
        Ok(())
    }

    #[test]
    fn isolated_mpv_profile_disables_user_config_scripts_and_osc() -> Result<(), VideoPreviewError>
    {
        let session = session()?;

        let args = session
            .mpv_args_with_profile(MpvLaunchProfile::Isolated)
            .join(" ");
        assert!(args.contains("127.0.0.1:39177"));
        assert!(!args.contains("baidu"));
        assert!(args.contains("--cache-on-disk=no"));
        assert!(args.contains("--no-config"));
        assert!(args.contains("--load-scripts=no"));
        assert!(args.contains("--osc=no"));
        assert!(args.contains("--no-terminal"));
        assert!(args.contains("--force-window=immediate"));
        assert!(args.contains("--msg-level=all=warn"));
        assert!(!args.contains("--really-quiet"));
        assert!(!args.contains("--vo=null"));
        assert!(!args.contains("--pause"));
        assert!(args.contains("--idle=yes"));
        assert!(args.contains("--demuxer-max-bytes=512"));
        Ok(())
    }

    #[test]
    fn player_log_ring_keeps_recent_sanitized_lines() {
        let mut log = super::PlayerLogRing::new(2);
        log.push("Opening http://127.0.0.1:1234/preview/video/tok_secret");
        log.push("A: 00:00:01 / 00:20:00");
        log.push("Error: https://d.pcs.baidu.com/file/private?token=SECRET");

        assert_eq!(log.lines().len(), 2);
        assert!(log.lines()[0].contains("A: 00:00:01"));
        assert!(log.lines()[1].contains("<redacted-url>"));
        assert!(!log.lines()[1].contains("SECRET"));
    }

    #[test]
    fn player_log_ring_redacts_token_like_fields() {
        let mut log = super::PlayerLogRing::new(2);
        log.push("headers BDUSS=SECRET_BDUSS token=SECRET_TOKEN cookie=SECRET_COOKIE");

        let line = log.last_line().unwrap_or_default();
        assert!(line.contains("BDUSS=<redacted>"));
        assert!(line.contains("token=<redacted>"));
        assert!(line.contains("cookie=<redacted>"));
        assert!(!line.contains("SECRET"));
    }

    #[test]
    fn interval_buffer_serves_overlapping_subranges() -> Result<(), VideoPreviewError> {
        let budget = VideoCacheBudget::new(16, 2)?;
        let mut buffer = VideoIntervalBuffer::new(budget);
        buffer.put(VideoRange::new(0, 15)?, b"abcdefghijklmnop".to_vec())?;

        assert_eq!(buffer.get(VideoRange::new(4, 7)?), Some(b"efgh".to_vec()));
        assert_eq!(buffer.get(VideoRange::new(12, 20)?), None);
        Ok(())
    }

    #[test]
    fn interval_buffer_evicts_until_within_budget() -> Result<(), VideoPreviewError> {
        let budget = VideoCacheBudget::new(8, 2)?;
        let mut buffer = VideoIntervalBuffer::new(budget);
        buffer.put(VideoRange::new(0, 7)?, b"aaaaaaaa".to_vec())?;
        buffer.put(VideoRange::new(8, 15)?, b"bbbbbbbb".to_vec())?;
        buffer.put(VideoRange::new(16, 23)?, b"cccccccc".to_vec())?;

        assert_eq!(buffer.used_bytes, 16);
        assert!(buffer.get(VideoRange::new(0, 7)?).is_none());
        assert_eq!(
            buffer.get(VideoRange::new(16, 23)?),
            Some(b"cccccccc".to_vec())
        );
        Ok(())
    }

    #[test]
    fn open_ended_proxy_range_uses_bounded_playback_chunk() -> Result<(), VideoPreviewError> {
        let session = session()?;
        let response = session.handle_proxy_request(&VideoProxyRequest {
            method: VideoProxyMethod::Get,
            token: "tok_0123456789abcdef".to_string(),
            range: Some(RequestedVideoRange::OpenEnded(100)),
        })?;

        assert_eq!(
            response,
            VideoProxyResponse::Fetch {
                range: VideoRange::new(100, 355)?,
                force_prefetch: true,
            }
        );
        Ok(())
    }

    #[test]
    fn explicit_proxy_range_can_use_full_cache_budget() -> Result<(), VideoPreviewError> {
        let session = session()?;
        let response = session.handle_proxy_request(&VideoProxyRequest {
            method: VideoProxyMethod::Get,
            token: "tok_0123456789abcdef".to_string(),
            range: Some(VideoRange::new(100, 900)?.into()),
        })?;

        assert_eq!(
            response,
            VideoProxyResponse::Fetch {
                range: VideoRange::new(100, 611)?,
                force_prefetch: false,
            }
        );
        Ok(())
    }

    #[test]
    fn suffix_proxy_range_resolves_against_file_tail() -> Result<(), VideoPreviewError> {
        let session = session()?;
        let response = session.handle_proxy_request(&VideoProxyRequest {
            method: VideoProxyMethod::Get,
            token: "tok_0123456789abcdef".to_string(),
            range: Some(RequestedVideoRange::Suffix(128)),
        })?;

        assert_eq!(
            response,
            VideoProxyResponse::Fetch {
                range: VideoRange::new(872, 999)?,
                force_prefetch: false,
            }
        );
        Ok(())
    }

    #[test]
    fn range_parser_accepts_open_ended_and_suffix_ranges() -> Result<(), VideoPreviewError> {
        assert_eq!(
            super::parse_video_range_header("Range: bytes=4096-").transpose()?,
            Some(RequestedVideoRange::OpenEnded(4096))
        );
        assert_eq!(
            super::parse_video_range_header("Range: bytes=-8192").transpose()?,
            Some(RequestedVideoRange::Suffix(8192))
        );
        Ok(())
    }

    #[test]
    fn proxy_request_requires_matching_token() -> Result<(), VideoPreviewError> {
        let session = session()?;

        let rejected = session.handle_proxy_request(&VideoProxyRequest {
            method: VideoProxyMethod::Get,
            token: "wrong".to_string(),
            range: Some(VideoRange::new(10, 20)?.into()),
        });
        assert_eq!(rejected, Err(VideoPreviewError::InvalidVideoToken));

        let range = VideoRange::new(10, 20)?;
        let accepted = session.handle_proxy_request(&VideoProxyRequest {
            method: VideoProxyMethod::Get,
            token: "tok_0123456789abcdef".to_string(),
            range: Some(range.into()),
        })?;
        assert_eq!(
            accepted,
            VideoProxyResponse::Fetch {
                range,
                force_prefetch: false,
            }
        );
        Ok(())
    }

    #[test]
    fn proxy_head_response_reports_range_capability_without_fetching_body(
    ) -> Result<(), VideoPreviewError> {
        let session = session()?;

        let response = session.handle_proxy_request(&VideoProxyRequest {
            method: VideoProxyMethod::Head,
            token: "tok_0123456789abcdef".to_string(),
            range: None,
        })?;

        assert_eq!(
            response,
            VideoProxyResponse::Head {
                content_length: 1_000,
                accept_ranges: true,
            }
        );
        Ok(())
    }
}
