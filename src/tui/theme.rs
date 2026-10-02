//! Colors and glyphs. Only the 16 ANSI colors are used so the UI follows the
//! terminal's own palette, light or dark; `NO_COLOR` drops color entirely.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use super::icons;
use crate::config::IconSet;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    color: bool,
    pub icons: IconSet,
}

impl Theme {
    pub fn new(icons: IconSet) -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        Self {
            color: !no_color,
            icons,
        }
    }

    const fn fg(self, color: Color) -> Style {
        if self.color {
            Style::new().fg(color)
        } else {
            Style::new()
        }
    }

    pub const fn text(self) -> Style {
        Style::new()
    }

    pub const fn dim(self) -> Style {
        if self.color {
            Style::new().fg(Color::DarkGray)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        }
    }

    pub const fn accent(self) -> Style {
        self.fg(Color::Cyan)
    }

    pub const fn title(self) -> Style {
        self.fg(Color::Cyan).add_modifier(Modifier::BOLD)
    }

    pub const fn error(self) -> Style {
        self.fg(Color::Red)
    }

    pub const fn success(self) -> Style {
        self.fg(Color::Green)
    }

    pub const fn warning(self) -> Style {
        self.fg(Color::Yellow)
    }

    pub const fn border(self) -> Style {
        self.dim()
    }

    /// The row under the cursor in the focused column.
    pub const fn cursor(self) -> Style {
        if self.color {
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::REVERSED)
        }
    }

    /// The cursor row when another pane has focus, and the current
    /// directory in the parent column.
    pub const fn cursor_inactive(self) -> Style {
        if self.color {
            Style::new()
                .fg(Color::Cyan)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::BOLD.union(Modifier::UNDERLINED))
        }
    }

    pub const fn marked(self) -> Style {
        self.fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }

    /// Mode badge at the left of the status bar.
    pub const fn badge(self, color: Color) -> Style {
        if self.color {
            Style::new()
                .fg(Color::Black)
                .bg(color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::REVERSED.union(Modifier::BOLD))
        }
    }

    /// A key name inside hints and dialogs.
    pub const fn key(self) -> Style {
        self.fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }

    /// Directory names; other names use the default text color, like yazi.
    pub const fn directory(self) -> Style {
        self.fg(Color::Blue)
    }

    /// Rounded end caps for a filled bar in `style`'s background color, like
    /// yazi's cursor and status pills. `None` without Nerd Font or color.
    pub fn caps(self, style: Style) -> Option<(Span<'static>, Span<'static>)> {
        let bg = style
            .bg
            .filter(|_| self.color && self.icons == IconSet::Nerd)?;
        let cap = |glyph| Span::styled(glyph, Style::new().fg(bg));
        Some((cap("\u{e0b6}"), cap("\u{e0b4}")))
    }

    /// `text` as a filled pill: rounded when possible, padded otherwise.
    pub fn pill(self, text: &str, style: Style) -> Vec<Span<'static>> {
        match self.caps(style) {
            Some((left, right)) => vec![left, Span::styled(text.to_string(), style), right],
            None => vec![Span::styled(format!(" {text} "), style)],
        }
    }

    /// Icon glyph (with trailing space) and its color for a file name; empty
    /// in ASCII mode. `open` shows a directory as an open folder.
    pub fn file_icon(self, name: &str, is_dir: bool, open: bool) -> (String, Style) {
        if matches!(self.icons, IconSet::Ascii) {
            return (String::new(), Style::new());
        }
        let icon = icons::icon_for(name, is_dir, open);
        let style = if self.color && !icon.is_fallback() {
            let [_, r, g, b] = icon.color.to_be_bytes();
            Style::new().fg(Color::Rgb(r, g, b))
        } else {
            Style::new()
        };
        (format!("{} ", icon.glyph), style)
    }

    /// Cells taken by an icon column.
    pub const fn icon_width(self) -> usize {
        match self.icons {
            IconSet::Nerd => 2,
            IconSet::Ascii => 0,
        }
    }

    pub const fn glyph(self, glyph: Glyph) -> &'static str {
        let nerd = matches!(self.icons, IconSet::Nerd);
        match glyph {
            Glyph::Cloud if nerd => "\u{f0c2} ",
            Glyph::Download if nerd => "\u{f019} ",
            Glyph::Open if nerd => "\u{f08e} ",
            Glyph::Remote if nerd => "\u{f0ec} ",
            Glyph::Search if nerd => "\u{f002} ",
            Glyph::Cloud => "",
            Glyph::Download => "v ",
            Glyph::Open => "> ",
            Glyph::Remote => "~ ",
            Glyph::Search => "? ",
            Glyph::Separator if nerd => " \u{f105} ",
            Glyph::Separator => "/",
            Glyph::Root if nerd => "\u{f0c2} ",
            Glyph::Root => "/",
        }
    }

    pub fn spinner(self, frame: usize) -> &'static str {
        const BRAILLE: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        const ASCII: [&str; 4] = ["|", "/", "-", "\\"];
        match self.icons {
            IconSet::Nerd => BRAILLE[frame % BRAILLE.len()],
            IconSet::Ascii => ASCII[frame % ASCII.len()],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Glyph {
    Cloud,
    Download,
    Open,
    Remote,
    Search,
    Separator,
    Root,
}
