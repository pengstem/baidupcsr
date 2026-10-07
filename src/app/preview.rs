use std::io::Cursor;
use std::num::NonZeroUsize;
use std::sync::Arc;

use bytes::Bytes;
use image::{DynamicImage, ImageReader, Limits};
use lru::LruCache;

use crate::pcs::FileEntry;

/// Bytes fetched from the start of a file for text and image previews.
pub const PREVIEW_FETCH_BYTES: u64 = 64 * 1024;
/// Largest thumbnail accepted; Baidu returns GIFs unscaled.
pub const THUMBNAIL_MAX_BYTES: u64 = 8 * 1024 * 1024;
const IMAGE_MAX_ALLOC: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    Directory,
    Video,
    Audio,
    Image,
    Text,
    Code,
    Subtitle,
    Archive,
    Document,
    Pdf,
    Ebook,
    Font,
    Other,
}

impl FileKind {
    pub fn of(entry: &FileEntry) -> Self {
        if entry.is_dir {
            Self::Directory
        } else {
            Self::from_name(&entry.server_filename)
        }
    }

    pub fn from_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "sha256sums" | "md5sums" | "checksums" | "readme" | "license"
        ) {
            return Self::Text;
        }
        let Some((_, extension)) = lower.rsplit_once('.') else {
            return Self::Other;
        };
        match extension {
            "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "flv" | "ts" | "rmvb" | "wmv" => {
                Self::Video
            }
            "mp3" | "flac" | "wav" | "m4a" | "aac" | "ogg" | "opus" | "ape" | "wma" | "alac" => {
                Self::Audio
            }
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "avif" | "tiff" => {
                Self::Image
            }
            "txt" | "md" | "markdown" | "log" | "csv" | "nfo" | "m3u" | "m3u8" | "cue" | "sfv"
            | "sha256" | "sha512" | "md5" => Self::Text,
            "rs" | "toml" | "json" | "yaml" | "yml" | "c" | "cc" | "cpp" | "h" | "hpp" | "py"
            | "js" | "tsx" | "jsx" | "html" | "css" | "xml" | "ini" | "conf" | "sh" | "bash"
            | "zsh" | "fish" | "go" | "java" | "kt" | "lua" => Self::Code,
            "srt" | "ass" | "ssa" | "vtt" | "sub" => Self::Subtitle,
            "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "iso" => {
                Self::Archive
            }
            "doc" | "docx" | "ppt" | "pptx" | "xls" | "xlsx" | "odt" | "ods" | "odp" | "rtf" => {
                Self::Document
            }
            "pdf" => Self::Pdf,
            "epub" | "mobi" | "azw3" | "fb2" => Self::Ebook,
            "ttf" | "otf" | "woff" | "woff2" => Self::Font,
            _ => Self::Other,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Image => "image",
            Self::Text => "text",
            Self::Code => "source code",
            Self::Subtitle => "subtitles",
            Self::Archive => "archive",
            Self::Document => "document",
            Self::Pdf => "PDF",
            Self::Ebook => "e-book",
            Self::Font => "font",
            Self::Other => "file",
        }
    }

    pub const fn is_text(self) -> bool {
        matches!(self, Self::Text | Self::Code | Self::Subtitle)
    }
}

/// Describe an image from its first bytes, e.g. `"PNG · 1920×1080"`.
pub fn describe_image(head: &[u8]) -> Option<String> {
    let kind = imagesize::image_type(head).ok()?;
    let size = imagesize::blob_size(head).ok()?;
    Some(format!("{kind:?} · {}×{}", size.width, size.height).to_uppercase())
}

/// Decode a thumbnail, refusing anything that would take unreasonable memory.
pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut limits = Limits::default();
    limits.max_alloc = Some(IMAGE_MAX_ALLOC);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    reader.limits(limits);
    reader.decode().map_err(|error| error.to_string())
}

/// Previews keyed by file identity, bounded by their total size.
pub struct PreviewCache<V> {
    cache: LruCache<(i64, i64), V>,
    used: usize,
    limit: usize,
}

/// Bytes a cached preview holds.
pub trait Weight {
    fn weight(&self) -> usize;
}

impl Weight for Arc<str> {
    fn weight(&self) -> usize {
        self.len()
    }
}

impl Weight for Bytes {
    fn weight(&self) -> usize {
        self.len()
    }
}

impl<V: Clone + Weight> PreviewCache<V> {
    pub fn new(limit: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::MIN.saturating_add(255)),
            used: 0,
            limit,
        }
    }

    pub fn get(&mut self, entry: &FileEntry) -> Option<V> {
        self.cache.get(&(entry.fs_id, entry.mtime)).cloned()
    }

    pub fn insert(&mut self, entry: &FileEntry, value: V) -> V {
        self.used += value.weight();
        if let Some((_, old)) = self.cache.push((entry.fs_id, entry.mtime), value.clone()) {
            self.used -= old.weight();
        }
        while self.used > self.limit {
            let Some((_, old)) = self.cache.pop_lru() else {
                break;
            };
            self.used -= old.weight();
        }
        value
    }
}

/// Decoded text previews.
pub type TextCache = PreviewCache<Arc<str>>;
/// Thumbnails as Baidu sent them (decoding is cheap next to the fetch).
pub type ThumbnailCache = PreviewCache<Bytes>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_from_names() {
        assert_eq!(FileKind::from_name("Lanterns.S01E01.MKV"), FileKind::Video);
        assert_eq!(FileKind::from_name("notes.md"), FileKind::Text);
        assert_eq!(FileKind::from_name("main.rs"), FileKind::Code);
        assert_eq!(FileKind::from_name("zh.ass"), FileKind::Subtitle);
        assert_eq!(FileKind::from_name("noext"), FileKind::Other);
    }

    #[test]
    fn text_cache_evicts_to_limit() {
        let mut cache = TextCache::new(10);
        let entry = |fs_id| FileEntry {
            fs_id,
            ..FileEntry::default()
        };
        cache.insert(&entry(1), "123456".into());
        cache.insert(&entry(2), "abcdef".into());
        assert!(cache.get(&entry(1)).is_none());
        assert_eq!(cache.get(&entry(2)).as_deref(), Some("abcdef"));
    }
}
