use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use headers::HeaderMapExt as _;
use reqwest::header::{COOKIE, RANGE, USER_AGENT};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use url::form_urlencoded;

use super::model::{
    locate_sign, ByteRange, ErrorFields, FileEntry, FileOperation, FileOperationPair, Ignored,
    ListResponse, LocateDownloadResponse, ParamList, PathParam, PcsError, Quota, QuotaResponse,
};

/// App id the official netdisk client uses for `locatedownload`.
const PAN_APP_ID: &str = "250528";

#[derive(Clone, Debug)]
pub struct PcsSettings {
    pub app_id: i64,
    pub pcs_addr: String,
    pub https: bool,
    /// User agent for `pcs.baidu.com` API calls.
    pub pcs_ua: String,
    /// User agent the download hosts expect (the netdisk client's).
    pub pan_ua: String,
    pub proxy: Option<String>,
}

/// Thin, cheaply clonable client for the Baidu PCS REST API.
#[derive(Clone, Debug)]
pub struct PcsClient {
    http: reqwest::Client,
    settings: PcsSettings,
    cookie: String,
}

impl PcsClient {
    pub fn new(settings: PcsSettings, cookie: String) -> Result<Self, PcsError> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(16);
        if let Some(proxy) = settings.proxy.as_deref().filter(|proxy| !proxy.is_empty()) {
            let proxy = reqwest::Proxy::all(proxy)
                .map_err(|error| PcsError::network("client_init", &error))?;
            builder = builder.proxy(proxy);
        }
        let http = builder
            .build()
            .map_err(|error| PcsError::network("client_init", &error))?;
        Ok(Self {
            http,
            settings,
            cookie,
        })
    }

    pub fn pan_user_agent(&self) -> &str {
        &self.settings.pan_ua
    }

    pub async fn list(
        &self,
        path: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<FileEntry>, PcsError> {
        let url = self.pcs_url(&[
            ("method", "list"),
            ("path", path),
            ("by", "name"),
            ("order", "asc"),
        ]);
        let response: ListResponse = self.json("list", self.get(&url), cancel).await?;
        Ok(response.list)
    }

    pub async fn search(
        &self,
        path: &str,
        keyword: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<FileEntry>, PcsError> {
        let url = self.pcs_url(&[
            ("method", "search"),
            ("path", path),
            ("wd", keyword),
            ("re", "1"),
        ]);
        let response: ListResponse = self.json("search", self.get(&url), cancel).await?;
        Ok(response.list)
    }

    pub async fn quota(&self, cancel: &CancellationToken) -> Result<Quota, PcsError> {
        let request = self
            .http
            .get("https://pan.baidu.com/api/quota?checkexpire=1&checkfree=1")
            .header(USER_AGENT, &self.settings.pan_ua)
            .header(COOKIE, &self.cookie);
        let response: QuotaResponse = self.json("quota", request, cancel).await?;
        Ok(Quota {
            total: response.total,
            used: response.used,
        })
    }

    /// Direct download links for `path`, best first. They only need the pan
    /// user agent (no cookie) and stay valid for about eight hours.
    pub async fn locate_download_urls(
        &self,
        path: &str,
        uid: u64,
        bduss: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, PcsError> {
        let sign = locate_sign(unix_now(), uid, bduss);
        let (time, rand) = (sign.time.to_string(), sign.rand);
        // No `ant=1`: it adds P2P "acceleration" mirrors (`*-ant.baidu.com`)
        // that redirect to nodes serving scrambled bytes.
        let url = self.url(&[
            ("check_blue", "1"),
            ("es", "1"),
            ("esl", "1"),
            ("app_id", PAN_APP_ID),
            ("method", "locatedownload"),
            ("path", path),
            ("ver", "4.0"),
            ("clienttype", "17"),
            ("channel", "0"),
            ("apn_id", "1_0"),
            ("freeisp", "0"),
            ("queryfree", "0"),
            ("use", "0"),
            ("time", &time),
            ("rand", &rand),
            ("devuid", &sign.devuid),
            ("cuid", &sign.devuid),
        ]);
        let request = self
            .http
            .post(&url)
            .header(USER_AGENT, &self.settings.pan_ua)
            .header(COOKIE, &self.cookie);
        let response: LocateDownloadResponse = self.json("locatedownload", request, cancel).await?;
        let urls: Vec<String> = response
            .urls
            .into_iter()
            .filter(|url| url.encrypt == 0 && !is_accelerated_mirror(&url.url))
            .map(|url| self.with_scheme(url.url))
            .collect();
        if urls.is_empty() {
            return Err(PcsError::parse("locatedownload", "no usable download url"));
        }
        Ok(urls)
    }

    /// Fetch exactly `range` from a located download URL.
    pub async fn fetch_range(
        &self,
        url: &str,
        range: ByteRange,
        cancel: &CancellationToken,
    ) -> Result<Bytes, PcsError> {
        const OP: &str = "download";
        let request = self
            .http
            .get(url)
            .header(USER_AGENT, &self.settings.pan_ua)
            .header(RANGE, range.header_value());
        let response = send(OP, request, cancel).await?;
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(PcsError::Network {
                operation: OP,
                message: format!("HTTP {}", response.status()),
            });
        }
        let content_range = response
            .headers()
            .typed_get::<headers::ContentRange>()
            .and_then(|value| value.bytes_range());
        if content_range != Some((range.start, range.end)) {
            return Err(PcsError::parse(OP, "Content-Range does not match request"));
        }
        let body = cancellable(OP, cancel, response.bytes()).await?;
        if body.len() as u64 != range.len() {
            return Err(PcsError::parse(
                OP,
                format!("got {} of {} bytes", body.len(), range.len()),
            ));
        }
        Ok(body)
    }

    pub async fn mkdir(&self, path: &str, cancel: &CancellationToken) -> Result<(), PcsError> {
        let url = self.pcs_url(&[("method", "mkdir"), ("path", path)]);
        let _: Ignored = self.json("mkdir", self.post(&url), cancel).await?;
        Ok(())
    }

    pub async fn delete(
        &self,
        paths: &[String],
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        let list: Vec<PathParam<'_>> = paths.iter().map(|path| PathParam { path }).collect();
        self.file_operation(FileOperation::Delete, &list, cancel)
            .await
    }

    pub async fn copy(
        &self,
        pairs: &[FileOperationPair],
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        self.file_operation(FileOperation::Copy, pairs, cancel)
            .await
    }

    pub async fn rename_or_move(
        &self,
        pairs: &[FileOperationPair],
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        self.file_operation(FileOperation::Move, pairs, cancel)
            .await
    }

    async fn file_operation<T: serde::Serialize + ?Sized>(
        &self,
        operation: FileOperation,
        list: &T,
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        let op = operation.method();
        let param = serde_json::to_string(&ParamList { list })
            .map_err(|error| PcsError::parse(op, error))?;
        let url = self.pcs_url(&[("method", op)]);
        let form = reqwest::multipart::Form::new().text("param", param);
        let _: Ignored = self
            .json(op, self.post(&url).multipart(form), cancel)
            .await?;
        Ok(())
    }

    fn get(&self, url: &str) -> RequestBuilder {
        self.with_auth(self.http.get(url))
    }

    fn post(&self, url: &str) -> RequestBuilder {
        self.with_auth(self.http.post(url))
    }

    fn with_auth(&self, request: RequestBuilder) -> RequestBuilder {
        request
            .header(USER_AGENT, &self.settings.pcs_ua)
            .header(COOKIE, &self.cookie)
    }

    fn pcs_url(&self, pairs: &[(&str, &str)]) -> String {
        let app_id = self.settings.app_id.to_string();
        let mut all = vec![("app_id", app_id.as_str())];
        all.extend_from_slice(pairs);
        self.url(&all)
    }

    fn url(&self, pairs: &[(&str, &str)]) -> String {
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs)
            .finish();
        format!(
            "{}://{}/rest/2.0/pcs/file?{query}",
            scheme(self.settings.https),
            self.settings.pcs_addr
        )
    }

    fn with_scheme(&self, raw: String) -> String {
        let Ok(mut url) = url::Url::parse(&raw) else {
            return raw;
        };
        match url.set_scheme(scheme(self.settings.https)) {
            Ok(()) => url.into(),
            Err(()) => raw,
        }
    }

    /// Send a request and decode its JSON body, turning Baidu's errno fields
    /// into [`PcsError::Remote`].
    async fn json<T: DeserializeOwned>(
        &self,
        operation: &'static str,
        request: RequestBuilder,
        cancel: &CancellationToken,
    ) -> Result<T, PcsError> {
        let response = send(operation, request, cancel).await?;
        let status = response.status();
        let body = cancellable(operation, cancel, response.bytes()).await?;
        if let Ok(fields) = serde_json::from_slice::<ErrorFields>(&body) {
            if let Some(error) = fields.into_error(operation) {
                return Err(error);
            }
        }
        if !status.is_success() {
            return Err(PcsError::Network {
                operation,
                message: format!("HTTP {status}"),
            });
        }
        serde_json::from_slice(&body).map_err(|error| PcsError::parse(operation, error))
    }
}

async fn send(
    operation: &'static str,
    request: RequestBuilder,
    cancel: &CancellationToken,
) -> Result<Response, PcsError> {
    cancellable(operation, cancel, request.send()).await
}

async fn cancellable<T>(
    operation: &'static str,
    cancel: &CancellationToken,
    future: impl std::future::Future<Output = reqwest::Result<T>>,
) -> Result<T, PcsError> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(PcsError::Cancelled { operation }),
        result = future => result.map_err(|error| PcsError::network(operation, &error)),
    }
}

fn is_accelerated_mirror(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.contains("-ant.")))
        .unwrap_or(false)
}

const fn scheme(https: bool) -> &'static str {
    if https {
        "https"
    } else {
        "http"
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::is_accelerated_mirror;

    #[test]
    fn skips_p2p_acceleration_mirrors() {
        assert!(is_accelerated_mirror(
            "http://d0-ant.baidu.com/file/x?fid=1"
        ));
        assert!(!is_accelerated_mirror(
            "https://allall02.baidupcs.com/file/x"
        ));
    }
}
