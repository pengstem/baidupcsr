//! Remote file operations, the `:` command language, and remote search.

use std::collections::{BTreeSet, VecDeque};

use tokio_util::sync::CancellationToken;

use super::remote_path;
use crate::pcs::{FileEntry, FileOperationPair, PcsClient, PcsError};

const SEARCH_BATCH: usize = 64;
const SEARCH_MAX_RESULTS: usize = 2_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    Mkdir(String),
    Delete(Vec<String>),
    Rename { from: String, to: String },
    Copy(Vec<FileOperationPair>),
    Move(Vec<FileOperationPair>),
}

impl Operation {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Mkdir(_) => "mkdir",
            Self::Delete(_) => "delete",
            Self::Rename { .. } => "rename",
            Self::Copy(_) => "copy",
            Self::Move(_) => "move",
        }
    }

    pub const fn is_destructive(&self) -> bool {
        matches!(self, Self::Delete(_))
    }

    /// One-line summary for task lists and toasts.
    pub fn summary(&self) -> String {
        match self {
            Self::Mkdir(path) => format!("mkdir {}", remote_path::file_name(path)),
            Self::Rename { from, to } => format!(
                "rename {} → {}",
                remote_path::file_name(from),
                remote_path::file_name(to)
            ),
            Self::Delete(paths) => count_summary("delete", paths.iter().map(String::as_str)),
            Self::Copy(pairs) => count_summary("copy", pairs.iter().map(|pair| pair.from.as_str())),
            Self::Move(pairs) => count_summary("move", pairs.iter().map(|pair| pair.from.as_str())),
        }
    }

    /// Every path the operation touches, for confirmation dialogs.
    pub fn details(&self) -> Vec<String> {
        match self {
            Self::Mkdir(path) => vec![path.clone()],
            Self::Delete(paths) => paths.clone(),
            Self::Rename { from, to } => vec![format!("{from} → {to}")],
            Self::Copy(pairs) | Self::Move(pairs) => pairs
                .iter()
                .map(|pair| format!("{} → {}", pair.from, pair.to))
                .collect(),
        }
    }

    /// Directories whose listings change when the operation succeeds.
    pub fn affected_dirs(&self) -> Vec<String> {
        let paths: Vec<&str> = match self {
            Self::Mkdir(path) => vec![path],
            Self::Delete(paths) => paths.iter().map(String::as_str).collect(),
            Self::Rename { from, to } => vec![from, to],
            Self::Copy(pairs) | Self::Move(pairs) => pairs
                .iter()
                .flat_map(|pair| [pair.from.as_str(), pair.to.as_str()])
                .collect(),
        };
        let dirs: BTreeSet<String> = paths.into_iter().map(remote_path::parent_or_root).collect();
        dirs.into_iter().collect()
    }

    pub async fn execute(
        &self,
        client: &PcsClient,
        cancel: &CancellationToken,
    ) -> Result<(), PcsError> {
        match self {
            Self::Mkdir(path) => client.mkdir(path, cancel).await,
            Self::Delete(paths) => client.delete(paths, cancel).await,
            Self::Rename { from, to } => {
                let pair = FileOperationPair {
                    from: from.clone(),
                    to: to.clone(),
                };
                client.rename_or_move(&[pair], cancel).await
            }
            Self::Copy(pairs) => client.copy(pairs, cancel).await,
            Self::Move(pairs) => client.rename_or_move(pairs, cancel).await,
        }
    }
}

fn count_summary<'a>(verb: &str, mut paths: impl ExactSizeIterator<Item = &'a str>) -> String {
    match paths.len() {
        1 => format!("{verb} {}", paths.next().map_or("", remote_path::file_name)),
        count => format!("{verb} {count} items"),
    }
}

/// Copy or move `paths` into `dir`, keeping their names.
pub fn transfer_into(paths: &[String], dir: &str, cut: bool) -> Result<Operation, String> {
    let pairs: Vec<FileOperationPair> = paths
        .iter()
        .map(|path| FileOperationPair {
            from: remote_path::normalize(path),
            to: remote_path::join(dir, remote_path::file_name(path)),
        })
        .filter(|pair| pair.from != pair.to)
        .collect();
    if pairs.is_empty() {
        return Err("nothing to paste here".to_string());
    }
    if let Some(pair) = pairs
        .iter()
        .find(|pair| pair.to.starts_with(&format!("{}/", pair.from)))
    {
        return Err(format!("cannot paste {} into itself", pair.from));
    }
    Ok(if cut {
        Operation::Move(pairs)
    } else {
        Operation::Copy(pairs)
    })
}

/// Rename `from` to `name` (or to an absolute path). `None` if unchanged.
pub fn rename(from: &str, name: &str) -> Result<Option<Operation>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name must not be empty".to_string());
    }
    let parent = remote_path::parent_or_root(from);
    let to = remote_path::resolve(&parent, name);
    Ok((to != from).then(|| Operation::Rename {
        from: from.to_string(),
        to,
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchMode {
    /// List every directory below the root and match names locally.
    Recursive,
    /// Only the root directory.
    Current,
    /// Baidu's server-side search.
    Api,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Run(Operation),
    Search { keyword: String, mode: SearchMode },
}

/// What a `:` command operates on.
pub struct CommandContext<'a> {
    pub cwd: &'a str,
    /// Marked entries, or the entry under the cursor.
    pub targets: Vec<String>,
}

pub fn parse_command(input: &str, ctx: &CommandContext<'_>) -> Result<Command, String> {
    let words = shell_words::split(input).map_err(|_| "unbalanced quotes".to_string())?;
    let Some((name, args)) = words.split_first() else {
        return Err("empty command".to_string());
    };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let resolve = |path: &str| remote_path::resolve(ctx.cwd, path);
    let targets = || {
        if ctx.targets.is_empty() {
            Err(format!("{name} needs a selected entry"))
        } else {
            Ok(ctx.targets.clone())
        }
    };

    let operation = match (name.as_str(), args.as_slice()) {
        ("mkdir", [path]) => Operation::Mkdir(resolve(path)),
        ("rename" | "ren", [name]) => {
            let target = targets()?;
            let [from] = target.as_slice() else {
                return Err("rename works on one entry".to_string());
            };
            rename(from, name)?.ok_or("name unchanged")?
        }
        ("rename" | "ren", [from, to]) => {
            rename(&resolve(from), &resolve(to))?.ok_or("name unchanged")?
        }
        ("cp" | "copy" | "mv" | "move", [sources @ .., dir]) => {
            let sources = if sources.is_empty() {
                targets()?
            } else {
                sources.iter().map(|path| resolve(path)).collect()
            };
            transfer_into(
                &sources,
                &resolve(dir),
                matches!(name.as_str(), "mv" | "move"),
            )?
        }
        ("rm" | "delete", []) => Operation::Delete(targets()?),
        ("rm" | "delete", paths) => {
            Operation::Delete(paths.iter().map(|path| resolve(path)).collect())
        }
        ("search", args) => {
            let mut mode = SearchMode::Recursive;
            let mut words = Vec::new();
            for arg in args {
                match *arg {
                    "-r" | "--recursive" => mode = SearchMode::Recursive,
                    "--current" => mode = SearchMode::Current,
                    "--api" => mode = SearchMode::Api,
                    word => words.push(word),
                }
            }
            let keyword = words.join(" ");
            if keyword.is_empty() {
                return Err("usage: search [--current|--api] <keyword>".to_string());
            }
            return Ok(Command::Search { keyword, mode });
        }
        ("mkdir", _) => return Err("usage: mkdir <path>".to_string()),
        ("rename" | "ren", _) => {
            return Err("usage: rename <new-name> | rename <from> <to>".to_string());
        }
        ("cp" | "copy" | "mv" | "move", _) => return Err(format!("usage: {name} [paths…] <dir>")),
        _ => return Err(format!("unknown command: {name}")),
    };
    Ok(Command::Run(operation))
}

/// Search below `root` for names containing `keyword`, streaming matches in
/// batches. Returns the total number of matches.
pub async fn search(
    client: &PcsClient,
    root: &str,
    keyword: &str,
    mode: SearchMode,
    mut on_batch: impl FnMut(Vec<FileEntry>),
    cancel: &CancellationToken,
) -> Result<usize, PcsError> {
    if mode == SearchMode::Api {
        let entries = client.search(root, keyword, cancel).await?;
        let count = entries.len();
        on_batch(entries);
        return Ok(count);
    }
    let needle = keyword.trim().to_lowercase();
    let mut queue = VecDeque::from([root.to_string()]);
    let mut matches = 0;
    while let Some(dir) = queue.pop_front() {
        let mut batch = Vec::new();
        for entry in client.list(&dir, cancel).await? {
            if mode == SearchMode::Recursive && entry.is_dir {
                queue.push_back(entry.path.clone());
            }
            if entry.server_filename.to_lowercase().contains(&needle) {
                matches += 1;
                batch.push(entry);
                if batch.len() >= SEARCH_BATCH {
                    on_batch(std::mem::take(&mut batch));
                }
                if matches >= SEARCH_MAX_RESULTS {
                    on_batch(batch);
                    return Ok(matches);
                }
            }
        }
        if !batch.is_empty() {
            on_batch(batch);
        }
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(targets: &[&str]) -> CommandContext<'static> {
        CommandContext {
            cwd: "/docs",
            targets: targets.iter().map(ToString::to_string).collect(),
        }
    }

    fn run(input: &str, targets: &[&str]) -> Operation {
        match parse_command(input, &ctx(targets)) {
            Ok(Command::Run(operation)) => operation,
            other => panic!("{input}: {other:?}"),
        }
    }

    #[test]
    fn parses_operations_relative_to_cwd() {
        assert_eq!(run("mkdir new", &[]), Operation::Mkdir("/docs/new".into()));
        assert_eq!(
            run("rename b.txt", &["/docs/a.txt"]),
            Operation::Rename {
                from: "/docs/a.txt".into(),
                to: "/docs/b.txt".into()
            }
        );
        assert_eq!(
            run("rm", &["/docs/a"]),
            Operation::Delete(vec!["/docs/a".into()])
        );
        let Operation::Move(pairs) = run("mv x '/my dir'", &[]) else {
            panic!("expected move");
        };
        assert_eq!(pairs[0].from, "/docs/x");
        assert_eq!(pairs[0].to, "/my dir/x");
    }

    #[test]
    fn reports_errors() {
        assert!(parse_command("rm", &ctx(&[])).is_err());
        assert!(parse_command("frobnicate", &ctx(&[])).is_err());
        assert!(parse_command("mkdir 'oops", &ctx(&[])).is_err());
        assert!(parse_command("rename a.txt", &ctx(&["/docs/a.txt"])).is_err());
    }

    #[test]
    fn parses_search_modes() {
        assert_eq!(
            parse_command("search --api foo bar", &ctx(&[])),
            Ok(Command::Search {
                keyword: "foo bar".into(),
                mode: SearchMode::Api
            })
        );
    }

    #[test]
    fn paste_rejects_noop_and_self_nesting() {
        assert!(transfer_into(&["/a/x".into()], "/a", false).is_err());
        assert!(transfer_into(&["/a".into()], "/a/b", true).is_err());
        let operation = transfer_into(&["/a/x".into(), "/a/y".into()], "/b", true).unwrap();
        assert_eq!(operation.summary(), "move 2 items");
        assert_eq!(operation.affected_dirs(), ["/a", "/b"]);
    }
}
