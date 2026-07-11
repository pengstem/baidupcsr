use super::*;

impl PcsTransportOptions {
    pub fn new_with_pan(
        user_agent: impl Into<String>,
        pan_user_agent: impl Into<String>,
        proxy: Option<String>,
    ) -> Self {
        Self {
            user_agent: user_agent.into(),
            pan_user_agent: pan_user_agent.into(),
            proxy,
            timeout: Duration::from_secs(15),
        }
    }
}

impl PcsClient {
    pub fn new(
        api_options: PcsClientOptions,
        auth: PcsAuth,
        options: PcsTransportOptions,
    ) -> Result<Self, PcsError> {
        let mut builder = reqwest::Client::builder()
            .timeout(options.timeout)
            .http1_only();
        if let Some(proxy) = options.proxy.as_ref().filter(|value| !value.is_empty()) {
            let proxy = reqwest::Proxy::all(proxy).map_err(|source| PcsError::Network {
                operation: "client_init".to_string(),
                message: reqwest_error_label(&source),
            })?;
            builder = builder.proxy(proxy);
        }
        let client = builder.build().map_err(|source| PcsError::Network {
            operation: "client_init".to_string(),
            message: reqwest_error_label(&source),
        })?;

        Ok(Self {
            api: PcsApi {
                options: api_options,
            },
            auth,
            options,
            client,
        })
    }

    pub async fn list(
        &self,
        path: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<FileEntry>, PcsError> {
        let request = self.api.list_request(path);
        let builder = self
            .client
            .get(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str());
        let response = send_request(builder, "list", cancel).await?;
        let status = response.status();
        let text = response_text(response, "list", cancel).await?;
        if !status.is_success() {
            return Err(parse_remote_error("list", &text));
        }
        parse_file_list_for_operation("list", &text)
    }

    pub async fn locate_download_urls(
        &self,
        path: &str,
        uid: u64,
        bduss: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, PcsError> {
        let sign = locatedownload_sign_with(now_unix_seconds(), &dev_uid(bduss), uid, bduss);
        let request = self.api.locatedownload_request(path, &sign);
        let builder = self
            .client
            .post(&request.url)
            .header(USER_AGENT, self.options.pan_user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str());
        let response = send_request(builder, "locatedownload", cancel).await?;
        let status = response.status();
        let text = response_text(response, "locatedownload", cancel).await?;
        if !status.is_success() {
            return Err(parse_remote_error("locatedownload", &text));
        }
        let info = parse_locatedownload(&text)?;
        let urls = select_download_urls(&info, self.api.options.enable_https);
        if urls.is_empty() {
            return Err(PcsError::Parse {
                operation: "locatedownload".to_string(),
                message: "no usable download url in response".to_string(),
            });
        }
        Ok(urls)
    }

    pub async fn mkdir(&self, path: &str, cancel: &CancellationToken) -> Result<(), PcsError> {
        let request = self.api.mkdir_request(path);
        self.send_file_operation_request("mkdir", &request, None, cancel)
            .await
    }

    pub async fn delete_paths(
        &self,
        paths: &[String],
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        let param = delete_param_json(paths)?;
        let request = self.api.file_operation_request(FileOperation::Delete);
        self.send_file_operation_request("delete", &request, Some(&param), cancel)
            .await
    }

    pub async fn copy_paths(
        &self,
        pairs: &[FileOperationPair],
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        let param = copy_move_param_json(pairs)?;
        let request = self.api.file_operation_request(FileOperation::Copy);
        self.send_file_operation_request("copy", &request, Some(&param), cancel)
            .await
    }

    pub async fn move_paths(
        &self,
        pairs: &[FileOperationPair],
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        let param = copy_move_param_json(pairs)?;
        let request = self.api.file_operation_request(FileOperation::Move);
        self.send_file_operation_request("move", &request, Some(&param), cancel)
            .await
    }

    pub async fn search(
        &self,
        path: &str,
        keyword: &str,
        recursive: bool,
        cancel: &CancellationToken,
    ) -> Result<Vec<FileEntry>, PcsError> {
        let request = self.api.search_request(path, keyword, recursive);
        let builder = self
            .client
            .get(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str());
        let response = send_request(builder, "search", cancel).await?;
        let status = response.status();
        let text = response_text(response, "search", cancel).await?;
        if !status.is_success() {
            return Err(parse_remote_error("search", &text));
        }
        parse_file_list_for_operation("search", &text)
    }

    pub async fn download_range(
        &self,
        path: &str,
        range: ByteRange,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, PcsError> {
        let request = self.api.download_request(path);
        let builder = self
            .client
            .get(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str())
            .header(RANGE, range.header_value());
        let response = send_request(builder, "download", cancel).await?;
        read_bounded_response(response, range, cancel).await
    }

    pub async fn download_url_range(
        &self,
        url: &str,
        range: ByteRange,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, PcsError> {
        let builder = self
            .client
            .get(url)
            .header(USER_AGENT, self.options.pan_user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str())
            .header(RANGE, range.header_value());
        let response = send_request(builder, "download", cancel).await?;
        read_bounded_response(response, range, cancel).await
    }

    pub(crate) async fn send_file_operation_request(
        &self,
        operation: &str,
        request: &PcsRequest,
        param: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        let mut builder = self
            .client
            .post(&request.url)
            .header(USER_AGENT, self.options.user_agent.as_str())
            .header(COOKIE, self.auth.cookie_header.as_str());
        if let Some(param) = param {
            let (content_type, body) = multipart_param_body(param);
            builder = builder.header(CONTENT_TYPE, content_type).body(body);
        }
        let response = send_request(builder, operation, cancel).await?;
        let status = response.status();
        let text = response_text(response, operation, cancel).await?;
        if !status.is_success() {
            return Err(parse_remote_error(operation, &text));
        }
        parse_file_operation_response(operation, &text)
    }
}

pub(crate) async fn send_request(
    builder: reqwest::RequestBuilder,
    operation: &str,
    cancel: &CancellationToken,
) -> Result<reqwest::Response, PcsError> {
    if cancel.is_cancelled() {
        return Err(cancelled(operation));
    }
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(cancelled(operation)),
        response = builder.send() => response.map_err(|source| PcsError::Network {
            operation: operation.to_string(),
            message: reqwest_error_label(&source),
        }),
    }
}

pub(crate) async fn response_text(
    response: reqwest::Response,
    operation: &str,
    cancel: &CancellationToken,
) -> Result<String, PcsError> {
    if cancel.is_cancelled() {
        return Err(cancelled(operation));
    }
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(cancelled(operation)),
        text = response.text() => text.map_err(|source| PcsError::Network {
            operation: operation.to_string(),
            message: reqwest_error_label(&source),
        }),
    }
}

pub(crate) fn cancelled(operation: &str) -> PcsError {
    PcsError::Cancelled {
        operation: operation.to_string(),
    }
}

pub(crate) fn reqwest_error_label(source: &reqwest::Error) -> String {
    let mut message = source.to_string();
    let mut next = std::error::Error::source(source);
    while let Some(cause) = next {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        next = cause.source();
    }
    message
}
