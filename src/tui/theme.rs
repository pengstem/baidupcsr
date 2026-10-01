//! Colors and glyphs. Only the 16 ANSI colors are used so the UI follows the
//! terminal's own palette, light or dark; `NO_COLOR` drops color entirely.

use ratatui::style::{Color, Modifier, Style};

use crate::app::preview::FileKind;
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

    pub const fn kind(self, kind: FileKind) -> Style {
        match kind {
            FileKind::Directory => self.fg(Color::Blue).add_modifier(Modifier::BOLD),
            FileKind::Video => self.fg(Color::Magenta),
            FileKind::Audio => self.fg(Color::Cyan),
            FileKind::Image => self.fg(Color::Yellow),
            FileKind::Archive => self.fg(Color::Red),
            FileKind::Document | FileKind::Pdf | FileKind::Ebook => self.fg(Color::Green),
            FileKind::Code | FileKind::Subtitle => self.fg(Color::LightBlue),
            FileKind::Text | FileKind::Font | FileKind::Other => Style::new(),
        }
    }

    /// Icon plus trailing space, or nothing in ASCII mode.
    pub const fn icon(self, kind: FileKind) -> &'static str {
        if matches!(self.icons, IconSet::Ascii) {
            return "";
        }
        match kind {
            FileKind::Directory => "\u{f07b} ",
            FileKind::Video => "\u{f1c8} ",
            FileKind::Audio => "\u{f1c7} ",
            FileKind::Image => "\u{f1c5} ",
            FileKind::Text => "\u{f15c} ",
            FileKind::Code => "\u{f1c9} ",
            FileKind::Subtitle => "\u{f20a} ",
            FileKind::Archive => "\u{f1c6} ",
            FileKind::Document => "\u{f1c2} ",
            FileKind::Pdf => "\u{f1c1} ",
            FileKind::Ebook => "\u{f02d} ",
            FileKind::Font => "\u{f031} ",
            FileKind::Other => "\u{f016} ",
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
