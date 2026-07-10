use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcsClientOptions {
    pub app_id: i64,
    pub pcs_addr: String,
    pub enable_https: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PcsApi {
    pub(crate) options: PcsClientOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PcsRequest {
    pub(crate) url: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileOperation {
    Delete,
    Copy,
    Move,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FileOperationPair {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    pub(crate) start: u64,
    pub(crate) end_inclusive: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcsAuth {
    pub(crate) cookie_header: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcsTransportOptions {
    pub user_agent: String,
    pub pan_user_agent: String,
    pub proxy: Option<String>,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct PcsClient {
    pub(crate) api: PcsApi,
    pub(crate) auth: PcsAuth,
    pub(crate) options: PcsTransportOptions,
    pub(crate) client: reqwest::Client,
}

impl PcsApi {
    pub(crate) fn list_request(&self, path: &str) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "list")
            .append_pair("path", path)
            .append_pair("by", "name")
            .append_pair("order", "asc")
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    pub(crate) fn download_request(&self, path: &str) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "download")
            .append_pair("path", path)
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    pub(crate) fn mkdir_request(&self, path: &str) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "mkdir")
            .append_pair("path", path)
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    pub(crate) fn search_request(&self, path: &str, keyword: &str, recursive: bool) -> PcsRequest {
        let recursive = if recursive { "1" } else { "0" };
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", "search")
            .append_pair("path", path)
            .append_pair("wd", keyword)
            .append_pair("re", recursive)
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    pub(crate) fn file_operation_request(&self, operation: FileOperation) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("app_id", &self.options.app_id.to_string())
            .append_pair("method", operation.method_name())
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }

    pub(crate) fn locatedownload_request(
        &self,
        path: &str,
        sign: &LocateDownloadSign,
    ) -> PcsRequest {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("ant", "1")
            .append_pair("check_blue", "1")
            .append_pair("es", "1")
            .append_pair("esl", "1")
            .append_pair("app_id", &PAN_APP_ID.to_string())
            .append_pair("method", "locatedownload")
            .append_pair("path", path)
            .append_pair("ver", "4.0")
            .append_pair("clienttype", "17")
            .append_pair("channel", "0")
            .append_pair("apn_id", "1_0")
            .append_pair("freeisp", "0")
            .append_pair("queryfree", "0")
            .append_pair("use", "0")
            .append_pair("time", &sign.time.to_string())
            .append_pair("rand", &sign.rand)
            .append_pair("devuid", &sign.devuid)
            .append_pair("cuid", &sign.devuid)
            .finish();

        PcsRequest {
            url: format!(
                "{}://{}/rest/2.0/pcs/file?{}",
                scheme(self.options.enable_https),
                self.options.pcs_addr,
                query
            ),
        }
    }
}
