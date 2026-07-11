#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PcsError {
    #[error("{operation}: cancelled")]
    Cancelled { operation: String },
    #[error("{operation}: failed to parse response: {message}")]
    Parse { operation: String, message: String },
    #[error("{operation}: network error: {message}")]
    Network { operation: String, message: String },
    #[error("{operation}: remote error {code}: {message}")]
    Remote {
        operation: String,
        code: i64,
        message: String,
    },
}

pub(crate) fn error_chain_message(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        let cause = error.to_string();
        if !cause.is_empty() && !message.ends_with(&cause) {
            message.push_str(": ");
            message.push_str(&cause);
        }
        source = error.source();
    }
    message
}
