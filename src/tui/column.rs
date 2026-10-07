use std::ops::Range;

use crate::pcs::FileEntry;

/// Rows kept visible above and below the cursor while scrolling.
const SCROLLOFF: usize = 3;

/// Cursor, scroll and filter state for the file list. Holds indices into a
/// listing owned elsewhere, so it must be rebuilt when that listing changes.
#[derive(Clone, Debug, Default)]
pub struct Column {
    filter: String,
    /// Indices of entries matching the filter, in listing order.
    visible: Vec<usize>,
    /// Position of the cursor within `visible`.
    cursor: usize,
    offset: usize,
    height: usize,
}

impl Column {
    /// Show a new listing, clearing the filter and selecting `select`.
    pub fn reset(&mut self, entries: &[FileEntry], select: Option<&str>) {
        self.filter.clear();
        self.cursor = 0;
        self.offset = 0;
        self.rebuild(entries, select);
    }

    /// Recompute matches after `entries` changed, keeping the cursor on
    /// `select` if it is still there, else near its old position.
    pub fn rebuild(&mut self, entries: &[FileEntry], select: Option<&str>) {
        let needle = self.filter.to_lowercase();
        self.visible = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                needle.is_empty() || entry.server_filename.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        if let Some(position) = select.and_then(|path| {
            self.visible
                .iter()
                .position(|&index| entries[index].path == path)
        }) {
            self.cursor = position;
        }
        self.cursor = self.cursor.min(self.visible.len().saturating_sub(1));
        self.scroll_into_view();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn set_filter(&mut self, filter: &str, entries: &[FileEntry]) {
        let selected = self.selected(entries).map(|entry| entry.path.clone());
        self.filter = filter.to_string();
        self.rebuild(entries, selected.as_deref());
    }

    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    pub const fn offset(&self) -> usize {
        self.offset
    }

    pub fn selected<'a>(&self, entries: &'a [FileEntry]) -> Option<&'a FileEntry> {
        self.visible
            .get(self.cursor)
            .and_then(|&index| entries.get(index))
    }

    /// Entry indices for visible positions in `range`.
    pub fn entries_in(&self, range: Range<usize>) -> &[usize] {
        let end = range.end.min(self.visible.len());
        &self.visible[range.start.min(end)..end]
    }

    /// Visible positions currently on screen.
    pub fn window(&self) -> Range<usize> {
        self.offset..(self.offset + self.height).min(self.visible.len())
    }

    pub fn set_height(&mut self, height: usize) {
        if self.height != height {
            self.height = height;
            self.scroll_into_view();
        }
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    /// Move the cursor; returns whether it moved.
    pub fn move_by(&mut self, delta: isize) -> bool {
        let target = self.cursor.saturating_add_signed(delta);
        self.move_to(target)
    }

    pub fn move_to(&mut self, position: usize) -> bool {
        let position = position.min(self.visible.len().saturating_sub(1));
        let moved = position != self.cursor;
        self.cursor = position;
        self.scroll_into_view();
        moved
    }

    /// Scroll the viewport without moving the cursor past the edges.
    pub fn scroll_by(&mut self, delta: isize) -> bool {
        let before = self.cursor;
        let max_offset = self.visible.len().saturating_sub(self.height);
        self.offset = self.offset.saturating_add_signed(delta).min(max_offset);
        let first = self.offset;
        let last = (self.offset + self.height).saturating_sub(1);
        self.cursor = self
            .cursor
            .clamp(first, last.max(first))
            .min(self.visible.len().saturating_sub(1));
        self.cursor != before
    }

    fn scroll_into_view(&mut self) {
        if self.height == 0 {
            return;
        }
        let margin = SCROLLOFF.min(self.height.saturating_sub(1) / 2);
        if self.cursor < self.offset + margin {
            self.offset = self.cursor.saturating_sub(margin);
        } else if self.cursor + margin >= self.offset + self.height {
            self.offset = self.cursor + margin + 1 - self.height;
        }
        self.offset = self
            .offset
            .min(self.visible.len().saturating_sub(self.height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(count: usize) -> Vec<FileEntry> {
        (0..count)
            .map(|index| FileEntry {
                path: format!("/f{index}"),
                server_filename: format!("f{index}"),
                ..FileEntry::default()
            })
            .collect()
    }

    #[test]
    fn keeps_scrolloff_margin() {
        let list = entries(50);
        let mut column = Column::default();
        column.set_height(10);
        column.reset(&list, None);
        column.move_to(6);
        assert_eq!(column.offset(), 0);
        column.move_to(7);
        assert_eq!(column.offset(), 1);
        column.move_to(49);
        assert_eq!(column.offset(), 40);
        column.move_by(-6);
        assert_eq!(column.offset(), 40);
        column.move_by(-1);
        assert_eq!(column.offset(), 39);
    }

    #[test]
    fn filter_keeps_selection_and_rebuild_follows_path() {
        let list = entries(20);
        let mut column = Column::default();
        column.set_height(5);
        column.reset(&list, Some("/f12"));
        assert_eq!(column.selected(&list).unwrap().path, "/f12");
        column.set_filter("1", &list);
        assert_eq!(column.len(), 11);
        assert_eq!(column.selected(&list).unwrap().path, "/f12");
        column.set_filter("nomatch", &list);
        assert!(column.selected(&list).is_none());
        column.set_filter("", &list);
        assert_eq!(column.len(), 20);
    }

    #[test]
    fn wheel_scroll_drags_cursor_along() {
        let list = entries(30);
        let mut column = Column::default();
        column.set_height(10);
        column.reset(&list, None);
        assert!(column.scroll_by(5));
        assert_eq!((column.offset(), column.cursor()), (5, 5));
        column.scroll_by(100);
        assert_eq!(column.offset(), 20);
    }
}
