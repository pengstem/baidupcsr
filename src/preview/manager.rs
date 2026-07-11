use super::*;

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

impl PreviewKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Pdf => "pdf",
            Self::Archive => "archive",
            Self::Audio => "audio",
            Self::Document => "document",
            Self::Ebook => "ebook",
            Self::Font => "font",
            Self::Directory => "directory",
            Self::Metadata => "metadata",
            Self::Video => "video",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewAction {
    FetchText,
    FetchImageMetadata,
    MetadataOnly,
}

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

    pub(crate) fn metadata(kind: PreviewKind, reason: &str) -> Self {
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

pub(crate) fn parse_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

pub(crate) fn parse_jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
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
    pub(crate) key: PreviewKey,
    pub(crate) generation: u64,
}

impl PreviewRequest {
    pub const fn key(self) -> PreviewKey {
        self.key
    }
}

#[derive(Clone, Debug)]
pub struct PreviewManager {
    pub(crate) memory_limit_bytes: u64,
    pub(crate) used_bytes: u64,
    pub(crate) cache: HashMap<PreviewKey, PreviewText>,
    pub(crate) lru: VecDeque<PreviewKey>,
    pub(crate) generations: HashMap<PreviewKey, u64>,
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

    pub(crate) fn text_status(&self, bytes: &[u8]) -> PreviewStatus {
        let incoming = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let fetched = incoming.min(self.memory_limit_bytes);
        PreviewStatus {
            kind: PreviewKind::Text,
            fetched_bytes: fetched,
            memory_limit_bytes: self.memory_limit_bytes,
        }
    }

    pub(crate) fn insert(&mut self, key: PreviewKey, text: PreviewText) {
        if let Some(old) = self.cache.remove(&key) {
            self.used_bytes = self.used_bytes.saturating_sub(old.status.fetched_bytes);
        }
        self.lru.retain(|item| item != &key);

        self.used_bytes = self.used_bytes.saturating_add(text.status.fetched_bytes);
        self.cache.insert(key, text);
        self.lru.push_back(key);
        self.evict_until_within_budget();
    }

    pub(crate) fn evict_until_within_budget(&mut self) {
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

pub(crate) const LOOPBACK_HOST: &str = "127.0.0.1";
pub(crate) const VIDEO_PREVIEW_PATH: &str = "/preview/video/";
