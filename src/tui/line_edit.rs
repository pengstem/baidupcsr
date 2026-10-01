use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Edit {
    Changed,
    Moved,
    Submit,
    Cancel,
    Ignored,
}

/// A single-line text field with readline-style keys.
#[derive(Clone, Debug, Default)]
pub struct LineEdit {
    text: String,
    /// Byte offset of the cursor, always on a char boundary.
    cursor: usize,
}

impl LineEdit {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cursor: text.len(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Display width of the text before the cursor.
    pub fn cursor_column(&self) -> usize {
        self.text[..self.cursor].width()
    }

    pub fn handle(&mut self, key: KeyEvent) -> Edit {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => return Edit::Submit,
            KeyCode::Esc => return Edit::Cancel,
            KeyCode::Char('c') if ctrl => return Edit::Cancel,
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.text.len(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            KeyCode::Left => self.cursor = self.prev_boundary(self.cursor),
            KeyCode::Right => self.cursor = self.next_boundary(),
            KeyCode::Char('u') if ctrl => {
                self.text.drain(..self.cursor);
                self.cursor = 0;
                return Edit::Changed;
            }
            KeyCode::Char('k') if ctrl => {
                self.text.truncate(self.cursor);
                return Edit::Changed;
            }
            KeyCode::Char('w') if ctrl => {
                let before = self.text[..self.cursor].trim_end();
                let start = before
                    .rfind(char::is_whitespace)
                    .map_or(0, |index| index + 1);
                self.text.drain(start..self.cursor);
                self.cursor = start;
                return Edit::Changed;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let start = self.prev_boundary(self.cursor);
                self.text.drain(start..self.cursor);
                self.cursor = start;
                return Edit::Changed;
            }
            KeyCode::Delete if self.cursor < self.text.len() => {
                let end = self.next_boundary();
                self.text.drain(self.cursor..end);
                return Edit::Changed;
            }
            KeyCode::Char(ch) if !ctrl => {
                self.text.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
                return Edit::Changed;
            }
            _ => return Edit::Ignored,
        }
        Edit::Moved
    }

    fn prev_boundary(&self, from: usize) -> usize {
        self.text[..from]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self) -> usize {
        self.text[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |ch| self.cursor + ch.len_utf8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    #[test]
    fn edits_multibyte_text_in_the_middle() {
        let mut edit = LineEdit::new("绿灯军团");
        edit.handle(key(KeyCode::Left));
        edit.handle(key(KeyCode::Backspace));
        assert_eq!(edit.text(), "绿灯团");
        edit.handle(key(KeyCode::Char('x')));
        assert_eq!(edit.text(), "绿灯x团");
        assert_eq!(edit.cursor_column(), 5);
    }

    #[test]
    fn readline_kills() {
        let mut edit = LineEdit::new("mv foo bar");
        assert_eq!(edit.handle(ctrl('w')), Edit::Changed);
        assert_eq!(edit.text(), "mv foo ");
        edit.handle(ctrl('a'));
        edit.handle(ctrl('k'));
        assert_eq!(edit.text(), "");
        assert_eq!(edit.handle(key(KeyCode::Enter)), Edit::Submit);
    }
}
