//! Per-file icons, matching yazi: exact directory and file names first, then
//! the extension.

use super::icon_table::{DIR, DIRS, EXTS, FILE, FILES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Icon {
    pub glyph: &'static str,
    /// 24-bit RGB.
    pub color: u32,
}

impl Icon {
    pub(super) const fn new(glyph: &'static str, color: u32) -> Self {
        Self { glyph, color }
    }

    /// The generic file icon, which carries no type information.
    pub fn is_fallback(self) -> bool {
        self == FILE
    }
}

fn find(table: &[(&str, Icon)], key: &str) -> Option<Icon> {
    table
        .binary_search_by(|(name, _)| (*name).cmp(key))
        .ok()
        .map(|index| table[index].1)
}

pub fn icon_for(name: &str, is_dir: bool) -> Icon {
    if is_dir {
        return find(DIRS, name).unwrap_or(DIR);
    }
    find(FILES, name)
        .or_else(|| {
            let (_, extension) = name.rsplit_once('.')?;
            find(EXTS, &extension.to_ascii_lowercase())
        })
        .unwrap_or(FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_for_binary_search() {
        for table in [DIRS, FILES, EXTS] {
            assert!(table.windows(2).all(|pair| pair[0].0 < pair[1].0));
        }
    }

    #[test]
    fn lookup_order() {
        assert_eq!(
            icon_for(".gitignore", false),
            find(FILES, ".gitignore").unwrap()
        );
        assert_eq!(icon_for("MOVIE.MKV", false), find(EXTS, "mkv").unwrap());
        assert_eq!(icon_for("Videos", true), find(DIRS, "Videos").unwrap());
        assert_eq!(icon_for("我的资源", true), DIR);
        assert!(icon_for("no-extension", false).is_fallback());
    }
}
