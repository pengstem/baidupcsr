use std::collections::{HashMap, VecDeque};

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
pub struct PreviewStatus {
    pub kind: PreviewKind,
    pub fetched_bytes: u64,
    pub memory_limit_bytes: u64,
    pub truncated: bool,
    pub disk_backed: bool,
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
