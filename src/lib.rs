//! baidupcsr: a terminal file manager for Baidu Netdisk.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod app;
pub mod cli;
pub mod config;
pub mod controller;
pub mod pcs;
pub mod tui;
