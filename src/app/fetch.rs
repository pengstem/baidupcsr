use std::future::Future;

use bytes::Bytes;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::pcs::{ByteRange, PcsClient, PcsError};

/// Who we are to Baidu: needed to sign `locatedownload` requests.
#[derive(Clone, Debug)]
pub struct Account {
    pub uid: u64,
    pub bduss: String,
}

pub trait RangeFetcher: Sync {
    fn fetch(
        &self,
        range: ByteRange,
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<Bytes, PcsError>> + Send;
}

/// Fetches byte ranges of one remote file from its located download URLs,
/// rotating to the next mirror on failure and re-locating once when every
/// mirror fails (links expire after a few hours).
pub struct RemoteFile {
    client: PcsClient,
    account: Account,
    path: String,
    urls: Mutex<Vec<String>>,
}

impl RemoteFile {
    pub fn new(client: PcsClient, account: Account, path: String) -> Self {
        Self {
            client,
            account,
            path,
            urls: Mutex::new(Vec::new()),
        }
    }

    /// The first mirror that serves a one-byte range; handed to the player.
    pub async fn working_url(&self, cancel: &CancellationToken) -> Result<String, PcsError> {
        let (url, _) = self.fetch_from_any(ByteRange::new(0, 0)?, cancel).await?;
        Ok(url)
    }

    async fn fetch_from_any(
        &self,
        range: ByteRange,
        cancel: &CancellationToken,
    ) -> Result<(String, Bytes), PcsError> {
        let mut last_error = None;
        for refresh in [false, true] {
            for url in self.urls(refresh, cancel).await? {
                match self.client.fetch_range(&url, range, cancel).await {
                    Ok(bytes) => {
                        self.promote(&url).await;
                        return Ok((url, bytes));
                    }
                    Err(error) if error.is_cancelled() => return Err(error),
                    Err(error) => last_error = Some(error),
                }
            }
        }
        Err(last_error.unwrap_or_else(|| PcsError::parse("download", "no download url")))
    }

    async fn urls(
        &self,
        refresh: bool,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, PcsError> {
        let mut urls = self.urls.lock().await;
        if refresh || urls.is_empty() {
            *urls = self
                .client
                .locate_download_urls(&self.path, self.account.uid, &self.account.bduss, cancel)
                .await?;
        }
        Ok(urls.clone())
    }

    async fn promote(&self, url: &str) {
        let mut urls = self.urls.lock().await;
        if let Some(index) = urls.iter().position(|candidate| candidate == url) {
            let url = urls.remove(index);
            urls.insert(0, url);
        }
    }
}

impl RangeFetcher for RemoteFile {
    async fn fetch(&self, range: ByteRange, cancel: &CancellationToken) -> Result<Bytes, PcsError> {
        Ok(self.fetch_from_any(range, cancel).await?.1)
    }
}
