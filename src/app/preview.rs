use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;

use crate::pcs::FileEntry;

/// Bytes fetched from the start of a file for text and image previews.
pub const PREVIEW_FETCH_BYTES: u64 = 64 * 1024;

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

/// Decoded text previews, bounded by total size.
pub struct TextCache {
    cache: LruCache<(i64, i64), Arc<str>>,
    used: usize,
    limit: usize,
}

impl TextCache {
    pub fn new(limit: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::MIN.saturating_add(255)),
            used: 0,
            limit,
        }
    }

    pub fn get(&mut self, entry: &FileEntry) -> Option<Arc<str>> {
        self.cache.get(&(entry.fs_id, entry.mtime)).cloned()
    }

    pub fn insert(&mut self, entry: &FileEntry, bytes: &[u8]) -> Arc<str> {
        let text: Arc<str> = String::from_utf8_lossy(bytes).into();
        self.used += text.len();
        if let Some((_, old)) = self
            .cache
            .push((entry.fs_id, entry.mtime), Arc::clone(&text))
        {
            self.used -= old.len();
        }
        while self.used > self.limit {
            let Some((_, old)) = self.cache.pop_lru() else {
                break;
            };
            self.used -= old.len();
        }
        text
    }
}

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
        cache.insert(&entry(1), b"123456");
        cache.insert(&entry(2), b"abcdef");
        assert!(cache.get(&entry(1)).is_none());
        assert_eq!(cache.get(&entry(2)).as_deref(), Some("abcdef"));
    }
}
