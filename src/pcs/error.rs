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
