//! Helpers for absolute, `/`-separated netdisk paths.

/// Make `path` absolute and strip trailing slashes (`"a/b/"` → `"/a/b"`).
pub fn normalize(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// The parent directory, or `None` for the root.
pub fn parent(path: &str) -> Option<String> {
    let path = normalize(path);
    if path == "/" {
        return None;
    }
    match path.rfind('/') {
        Some(0) | None => Some("/".to_string()),
        Some(index) => Some(path[..index].to_string()),
    }
}

/// The parent directory; the root is its own parent.
pub fn parent_or_root(path: &str) -> String {
    parent(path).unwrap_or_else(|| "/".to_string())
}

pub fn join(dir: &str, name: &str) -> String {
    let dir = normalize(dir);
    let name = name.trim_matches('/');
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Resolve `path` against `cwd` unless it is already absolute.
pub fn resolve(cwd: &str, path: &str) -> String {
    if path.starts_with('/') {
        normalize(path)
    } else {
        join(cwd, path)
    }
}

pub fn file_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("/")
}

/// The direct child of `ancestor` on the way to `descendant`, if any:
/// `child_toward("/a", "/a/b/c") == Some("/a/b")`.
pub fn child_toward(ancestor: &str, descendant: &str) -> Option<String> {
    let ancestor = normalize(ancestor);
    let descendant = normalize(descendant);
    let rest = if ancestor == "/" {
        descendant.strip_prefix('/')?
    } else {
        descendant.strip_prefix(&ancestor)?.strip_prefix('/')?
    };
    let child = rest.split('/').next().filter(|child| !child.is_empty())?;
    Some(join(&ancestor, child))
}

/// Make a remote file name safe to use as a local file name.
pub fn local_file_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if matches!(ch, '/' | '\\') || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect();
    if matches!(sanitized.as_str(), "" | "." | "..") {
        "download".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handles_root_relative_and_trailing() {
        assert_eq!(normalize(""), "/");
        assert_eq!(normalize("/"), "/");
        assert_eq!(normalize("a/b/"), "/a/b");
        assert_eq!(normalize("/我的资源//"), "/我的资源");
    }

    #[test]
    fn parent_walks_up_to_root() {
        assert_eq!(parent("/a/b").as_deref(), Some("/a"));
        assert_eq!(parent("/a").as_deref(), Some("/"));
        assert_eq!(parent("/"), None);
    }

    #[test]
    fn join_and_resolve() {
        assert_eq!(join("/", "x"), "/x");
        assert_eq!(join("/a/", "/x/"), "/a/x");
        assert_eq!(resolve("/a", "b"), "/a/b");
        assert_eq!(resolve("/a", "/b/"), "/b");
    }

    #[test]
    fn file_name_and_child_toward() {
        assert_eq!(file_name("/a/视频.mkv"), "视频.mkv");
        assert_eq!(file_name("/"), "/");
        assert_eq!(child_toward("/", "/a/b").as_deref(), Some("/a"));
        assert_eq!(child_toward("/a", "/a/b/c").as_deref(), Some("/a/b"));
        assert_eq!(child_toward("/a", "/ab/c"), None);
        assert_eq!(child_toward("/a", "/a"), None);
    }

    #[test]
    fn local_file_name_strips_separators() {
        assert_eq!(local_file_name("a/b\\c"), "a_b_c");
        assert_eq!(local_file_name(".."), "download");
    }
}
