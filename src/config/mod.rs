use std::collections::BTreeMap;

#[cfg(test)]
mod tests;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

const CONFIG_FILE_NAME: &str = "pcs_config.json";
const ENV_CONFIG_DIR: &str = "BAIDUPCS_GO_CONFIG_DIR";
const UI_CONFIG_FILE_NAME: &str = "ui_config.toml";
const ENV_UI_CONFIG: &str = "BAIDUPCSR_UI_CONFIG";
const ENV_CACHE_DIR: &str = "BAIDUPCSR_CACHE_DIR";

mod discovery;
mod model;

pub(crate) use discovery::*;
pub(crate) use model::*;
