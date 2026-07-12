use bytes::Bytes;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[allow(
    dead_code,
    reason = "legacy validation errors remain part of the test contract"
)]
pub enum VideoPreviewError {
    #[error("invalid video token format")]
    InvalidVideoTokenFormat,
    #[error("invalid cache budget")]
    InvalidCacheBudget,
    #[error("invalid range")]
    InvalidRange,
    #[error("invalid video token")]
    InvalidVideoToken,
    #[error("inactive video session")]
    InactiveVideoSession,
    #[error("video chunk too large")]
    VideoChunkTooLarge,
    #[error("loopback server I/O error")]
    LoopbackServerIo,
    #[error("malformed HTTP request")]
    MalformedHttpRequest,
    #[error("unsupported HTTP method")]
    UnsupportedHttpMethod,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum VideoError {
    #[error("invalid video token")]
    InvalidToken,
    #[error("invalid video range")]
    InvalidRange,
    #[error("video session is inactive")]
    Cancelled,
    #[error("video upstream failed: {0}")]
    Upstream(String),
    #[error("video loopback I/O failed: {0}")]
    Io(String),
    #[error("malformed HTTP request")]
    MalformedRequest,
    #[error("unsupported HTTP method")]
    UnsupportedMethod,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoToken {
    pub(crate) token: String,
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

    pub(crate) fn as_str(&self) -> &str {
        &self.token
    }

    pub(crate) fn matches(&self, candidate: &str) -> bool {
        self.token == candidate
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoCacheBudget {
    pub(crate) chunk_size_bytes: u64,
    pub(crate) max_chunks: usize,
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

    pub(crate) fn memory_limit_bytes(&self) -> u64 {
        self.chunk_size_bytes
            .saturating_mul(u64::try_from(self.max_chunks).unwrap_or(u64::MAX))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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

    pub(crate) const fn len(&self) -> u64 {
        if self.is_empty() {
            0
        } else {
            self.end_inclusive
                .saturating_sub(self.start)
                .saturating_add(1)
        }
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.start > self.end_inclusive
    }

    pub(crate) const fn contains(&self, other: Self) -> bool {
        self.start <= other.start && self.end_inclusive >= other.end_inclusive
    }

    pub(crate) fn clamp_to_file(self, file_size: u64) -> Result<Self, VideoPreviewError> {
        if file_size == 0 || self.start >= file_size {
            return Err(VideoPreviewError::InvalidRange);
        }

        Ok(Self {
            start: self.start,
            end_inclusive: self.end_inclusive.min(file_size - 1),
        })
    }

    pub(crate) fn clamp_len(self, max_len: u64) -> Result<Self, VideoPreviewError> {
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
pub(crate) enum RequestedVideoRange {
    Explicit(VideoRange),
    OpenEnded(u64),
    Suffix(u64),
}

impl RequestedVideoRange {
    pub(crate) const fn suffix(bytes: u64) -> Result<Self, VideoPreviewError> {
        if bytes == 0 {
            return Err(VideoPreviewError::InvalidRange);
        }
        Ok(Self::Suffix(bytes))
    }

    pub(crate) fn resolve(
        self,
        file_size: u64,
        max_len: u64,
    ) -> Result<VideoRange, VideoPreviewError> {
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
pub(crate) struct CachedInterval {
    pub(crate) range: VideoRange,
    pub(crate) bytes: Bytes,
    pub(crate) last_used: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct VideoIntervalBuffer {
    pub(crate) budget: VideoCacheBudget,
    pub(crate) used_bytes: u64,
    pub(crate) tick: u64,
    pub(crate) intervals: Vec<CachedInterval>,
}

impl VideoIntervalBuffer {
    pub(crate) const fn new(budget: VideoCacheBudget) -> Self {
        Self {
            budget,
            used_bytes: 0,
            tick: 0,
            intervals: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn put(
        &mut self,
        range: VideoRange,
        bytes: Vec<u8>,
    ) -> Result<(), VideoPreviewError> {
        self.put_bytes(range, Bytes::from(bytes))
    }

    pub(crate) fn put_bytes(
        &mut self,
        range: VideoRange,
        bytes: Bytes,
    ) -> Result<(), VideoPreviewError> {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if len != range.len() || len > self.budget.memory_limit_bytes() {
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

    #[cfg(test)]
    pub(crate) fn get(&mut self, range: VideoRange) -> Option<Vec<u8>> {
        self.get_bytes(range).map(|bytes| bytes.to_vec())
    }

    pub(crate) fn get_bytes(&mut self, range: VideoRange) -> Option<Bytes> {
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
        let bytes = self.intervals[index].bytes.slice(offset..end);
        self.intervals[index].last_used = self.next_tick();
        Some(bytes)
    }

    pub(crate) fn contains(&self, range: VideoRange) -> bool {
        self.intervals
            .iter()
            .any(|interval| interval.range.contains(range))
    }

    pub(crate) fn window(&self) -> Option<VideoRange> {
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

    pub(crate) fn remove_exact(&mut self, range: VideoRange) {
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

    pub(crate) fn evict_until_within_budget(&mut self) {
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

    pub(crate) fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }
}
