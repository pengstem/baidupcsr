use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteOperationKind {
    Mkdir {
        path: String,
    },
    Delete {
        paths: Vec<String>,
    },
    Rename {
        from: String,
        to: String,
    },
    Copy {
        pairs: Vec<crate::pcs::FileOperationPair>,
    },
    Move {
        pairs: Vec<crate::pcs::FileOperationPair>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteOperationPlan {
    kind: RemoteOperationKind,
    affected_dirs: Vec<String>,
    dry_run_lines: Vec<String>,
}

impl RemoteOperationPlan {
    pub fn mkdir(path: impl Into<String>) -> Result<Self, String> {
        let path = path.into();
        let path = normalize_remote_path(&path)?;
        let affected_dirs = related_dirs_for_paths([path.as_str()]);
        Ok(Self {
            dry_run_lines: vec![format!("mkdir {path}")],
            kind: RemoteOperationKind::Mkdir { path },
            affected_dirs,
        })
    }

    pub fn delete(paths: Vec<String>) -> Result<Self, String> {
        let paths = normalize_nonempty_paths(paths)?;
        let mut dry_run_lines = vec![format!("delete {} remote entries", paths.len())];
        dry_run_lines.extend(paths.iter().cloned());
        let affected_dirs = related_dirs_for_paths(paths.iter().map(String::as_str));
        Ok(Self {
            kind: RemoteOperationKind::Delete { paths },
            affected_dirs,
            dry_run_lines,
        })
    }

    pub fn rename(from: impl Into<String>, to: impl Into<String>) -> Result<Self, String> {
        let from = from.into();
        let to = to.into();
        let from = normalize_remote_path(&from)?;
        let to = normalize_remote_path(&to)?;
        let affected_dirs = related_dirs_for_paths([from.as_str(), to.as_str()]);
        Ok(Self {
            dry_run_lines: vec![format!("rename {from} -> {to}")],
            kind: RemoteOperationKind::Rename { from, to },
            affected_dirs,
        })
    }

    pub fn copy(pairs: Vec<crate::pcs::FileOperationPair>) -> Result<Self, String> {
        Self::copy_or_move("copy", pairs, false)
    }

    pub fn move_paths(pairs: Vec<crate::pcs::FileOperationPair>) -> Result<Self, String> {
        Self::copy_or_move("move", pairs, true)
    }

    pub const fn kind(&self) -> &RemoteOperationKind {
        &self.kind
    }

    pub const fn is_destructive(&self) -> bool {
        matches!(self.kind, RemoteOperationKind::Delete { .. })
    }

    pub fn affected_dirs(&self) -> &[String] {
        &self.affected_dirs
    }

    pub fn dry_run_lines(&self) -> &[String] {
        &self.dry_run_lines
    }

    pub const fn label(&self) -> &'static str {
        match &self.kind {
            RemoteOperationKind::Mkdir { .. } => "mkdir",
            RemoteOperationKind::Delete { .. } => "delete",
            RemoteOperationKind::Rename { .. } => "rename",
            RemoteOperationKind::Copy { .. } => "copy",
            RemoteOperationKind::Move { .. } => "move",
        }
    }

    fn copy_or_move(
        label: &'static str,
        pairs: Vec<crate::pcs::FileOperationPair>,
        is_move: bool,
    ) -> Result<Self, String> {
        let pairs = normalize_nonempty_pairs(pairs)?;
        let mut dry_run_lines = vec![format!("{label} {} remote entries", pairs.len())];
        dry_run_lines.extend(
            pairs
                .iter()
                .map(|pair| format!("{} -> {}", pair.from, pair.to)),
        );
        let affected_dirs = related_dirs_for_paths(
            pairs
                .iter()
                .flat_map(|pair| [pair.from.as_str(), pair.to.as_str()]),
        );
        Ok(Self {
            kind: if is_move {
                RemoteOperationKind::Move { pairs }
            } else {
                RemoteOperationKind::Copy { pairs }
            },
            affected_dirs,
            dry_run_lines,
        })
    }
}

fn normalize_nonempty_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("operation requires at least one remote path".to_string());
    }
    paths
        .into_iter()
        .map(|path| normalize_remote_path(path.as_str()))
        .collect()
}

fn normalize_nonempty_pairs(
    pairs: Vec<crate::pcs::FileOperationPair>,
) -> Result<Vec<crate::pcs::FileOperationPair>, String> {
    if pairs.is_empty() {
        return Err("operation requires at least one remote path pair".to_string());
    }
    pairs
        .into_iter()
        .map(|pair| {
            Ok(crate::pcs::FileOperationPair {
                from: normalize_remote_path(pair.from.as_str())?,
                to: normalize_remote_path(pair.to.as_str())?,
            })
        })
        .collect()
}

fn normalize_remote_path(path: &str) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("remote path must not be empty".to_string());
    }
    let mut normalized = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    Ok(normalized)
}

fn related_dirs_for_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let dirs: BTreeSet<String> = paths
        .into_iter()
        .map(|p| crate::tui::TuiApp::parent_path(p).unwrap_or_else(|| "/".to_string()))
        .collect();
    dirs.into_iter().collect()
}
