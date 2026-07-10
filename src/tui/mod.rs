use crate::pcs::FileEntry;
use crate::preview::PreviewKind;

#[cfg(test)]
mod tests;
use ratatui::{
    crossterm::{
        event::{
            DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyCode,
            KeyModifiers, MouseButton, MouseEventKind,
        },
        execute,
    },
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Cell, Clear, Gauge, HighlightSpacing, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableState, Wrap,
    },
    Frame,
};
use std::{cmp::Ordering, collections::BTreeMap, io, ops::Range, time::Duration};
use time::{macros::format_description, OffsetDateTime};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const THEME_BG: Color = Color::Rgb(15, 23, 42);
const THEME_PANEL: Color = Color::Rgb(24, 32, 52);
const THEME_BORDER: Color = Color::Rgb(51, 65, 85);
const THEME_BORDER_ACTIVE: Color = Color::Rgb(34, 211, 238);
const THEME_TEXT: Color = Color::Rgb(226, 232, 240);
const THEME_STATUS_TEXT: Color = Color::Rgb(203, 213, 225);
const THEME_MUTED: Color = Color::Rgb(148, 163, 184);
const THEME_DIM: Color = Color::Rgb(100, 116, 139);
const THEME_ACCENT: Color = Color::Rgb(94, 234, 212);
const THEME_SELECTED: Color = Color::Rgb(14, 116, 144);
const THEME_MARKED: Color = Color::Rgb(30, 41, 59);
const THEME_DIRECTORY: Color = Color::Rgb(125, 211, 252);
const THEME_WARNING: Color = Color::Rgb(250, 204, 21);

const THEME_SUCCESS: Color = Color::Rgb(52, 211, 153);
const THEME_ERROR: Color = Color::Rgb(248, 113, 113);
const PAGE_STEP: usize = 10;
const HALF_PAGE_STEP: usize = 5;
const MOUSE_WHEEL_STEP: usize = 3;
const DOUBLE_CLICK_TICKS: u64 = 5;
const MARQUEE_EDGE_PAUSE_TICKS: u64 = 8;
const HEADER_BRAND: &str = "baidupcsr";
const HEADER_GAP: &str = "  ";
const BREADCRUMB_SEPARATOR: &str = " > ";

mod layout;
mod render;
mod runtime;
mod state;

pub(crate) use layout::*;
pub(crate) use render::*;
pub(crate) use runtime::*;
pub(crate) use state::*;
