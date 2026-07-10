use super::*;

impl ByteRange {
    pub fn new(start: u64, end_inclusive: u64) -> Result<Self, PcsError> {
        if start > end_inclusive {
            return Err(PcsError::Network {
                operation: "download".to_string(),
                message: "range start must be before range end".to_string(),
            });
        }

        Ok(Self {
            start,
            end_inclusive,
        })
    }

    pub fn prefix(length: u64) -> Result<Self, PcsError> {
        if length == 0 {
            return Err(PcsError::Network {
                operation: "download".to_string(),
                message: "range length must be greater than zero".to_string(),
            });
        }

        Ok(Self {
            start: 0,
            end_inclusive: length - 1,
        })
    }

    pub const fn start(self) -> u64 {
        self.start
    }

    pub const fn end_inclusive(self) -> u64 {
        self.end_inclusive
    }

    pub(crate) fn header_value(&self) -> String {
        format!("bytes={}-{}", self.start, self.end_inclusive)
    }

    pub(crate) const fn read_limit_bytes(&self) -> u64 {
        self.end_inclusive
            .saturating_sub(self.start)
            .saturating_add(1)
    }
}

impl PcsAuth {
    pub fn from_cookie_header(cookie_header: impl Into<String>) -> Self {
        Self {
            cookie_header: cookie_header.into(),
        }
    }

    pub fn from_bduss_stoken(
        bduss: &str,
        stoken: Option<&str>,
        baiduid: Option<&str>,
    ) -> Option<Self> {
        if bduss.is_empty() {
            return None;
        }

        let mut parts = vec![format!("BDUSS={bduss}")];
        if let Some(stoken) = stoken.filter(|value| !value.is_empty()) {
            parts.push(format!("STOKEN={stoken}"));
        }
        if let Some(baiduid) = baiduid.filter(|value| !value.is_empty()) {
            parts.push(format!("BAIDUID={baiduid}"));
        }

        Some(Self {
            cookie_header: parts.join("; "),
        })
    }
}
