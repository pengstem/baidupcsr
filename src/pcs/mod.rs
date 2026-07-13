use std::time::{Duration, UNIX_EPOCH};

#[cfg(test)]
mod tests;

use futures_util::StreamExt;
use md5::{Digest as Md5Digest, Md5};
use reqwest::header::{COOKIE, RANGE, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha1::{Digest as Sha1Digest, Sha1};
use tokio_util::sync::CancellationToken;
use url::form_urlencoded;

const PAN_APP_ID: i64 = 250_528;

mod client;
mod error;
mod model;
mod request;
mod response;

pub(crate) use client::*;
pub(crate) use error::*;
pub(crate) use request::*;
pub(crate) use response::*;
