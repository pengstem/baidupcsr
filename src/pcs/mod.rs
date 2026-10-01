//! Baidu PCS REST API client.

mod client;
mod model;

pub use client::{PcsClient, PcsSettings};
pub use model::{ByteRange, FileEntry, FileOperationPair, PcsError, Quota};
