use std::collections::{HashMap, VecDeque};

#[cfg(test)]
mod tests;
use std::env;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;

const PREVIEW_MEMORY_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
const TEXT_PREVIEW_FETCH_BYTES: u64 = 64 * 1024;
const LOCAL_OPEN_MAX_BYTES: u64 = 100 * 1024 * 1024;
const LOCAL_OPEN_TEMP_DIR_NAME: &str = "baidupcsr-open";
const VIDEO_PREFETCH_CHUNK_LIMIT: usize = 31;
const MPV_PROGRAM: &str = "mpv";
const ENV_DOWNLOAD_DIR: &str = "BAIDUPCSR_DOWNLOAD_DIR";
const ENV_PROXY: &str = "BAIDUPCSR_PROXY";
const SEARCH_BATCH_SIZE: usize = 64;
const SEARCH_MAX_RESULTS: usize = 2_000;
const APP_EVENT_CAPACITY: usize = 256;

mod command;
mod doctor;
mod remote_operation;
mod runtime;
mod transfer;
mod video_bench;

pub(crate) use command::*;
pub(crate) use doctor::*;
pub(crate) use remote_operation::*;
pub(crate) use runtime::*;
pub(crate) use transfer::*;
pub(crate) use video_bench::*;
