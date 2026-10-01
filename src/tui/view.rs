//! Drawing. Layout: a one-line header, Miller columns (parent │ current │
//! preview), a transfer dock while downloads run, and a one-line status bar.

use std::time::Duration;

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::app::{App, Focus, InputKind, Overlay, PreviewBody, Regions, ToastLevel};
use super::format;
use super::keymap::HELP;
use super::theme::{Glyph, Theme};
use crate::app::directory::SortKey;
use crate::app::preview::FileKind;
use crate::app::remote_path;
use crate::app::task::{Task, TaskKind, TaskState};
use crate::config::IconSet;
use crate::pcs::FileEntry;

const MARQUEE_DELAY: Duration = Duration::from_secs(1);
const MARQUEE_STEP_MS: u128 = 150;
const SIZE_WIDTH: usize = 9;
const DOCK_MAX: usize = 3;

struct Areas {
    header: Rect,
    parent: Rect,
    files: Rect,
    preview: Rect,
    separators: Vec<Rect>,
    dock: Rect,
    status: Rect,
}

fn areas(area: Rect, app: &App) -> Areas {
    let dock_rows = app.tasks.active().count().min(DOCK_MAX) as u16;
    let [header, body, dock, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(dock_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    let (parent, files, preview, separators) = if body.width >= 100 {
        let [parent, sep1, files, sep2, preview] = Layout::horizontal([
            Constraint::Ratio(1, 7),
            Constraint::Length(1),
            Constraint::Ratio(3, 7),
            Constraint::Length(1),
            Constraint::Ratio(3, 7),
        ])
        .areas(body);
        (parent, files, preview, vec![sep1, sep2])
    } else if body.width >= 60 {
        let [files, sep, preview] = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .areas(body);
        (Rect::default(), files, preview, vec![sep])
    } else {
        (Rect::default(), body, Rect::default(), Vec::new())
    };
    Areas {
        header,
        parent,
        files,
        preview,
        separators,
        dock,
        status,
    }
}

/// Draw a frame and record its layout in `app` for mouse handling.
pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let areas = areas(frame.area(), app);
    let (header, crumbs) = header(app, areas.header);
    app.regions = Regions {
        parent: areas.parent,
        files: areas.files,
        preview: areas.preview,
        crumbs,
    };
    app.column.set_height(usize::from(areas.files.height));
    let app = &*app;
    let theme = app.theme;

    frame.render_widget(header, areas.header);
    for separator in &areas.separators {
        let line = Paragraph::new(vec![Line::from("│"); usize::from(separator.height)])
            .style(theme.border());
        frame.render_widget(line, *separator);
    }
    if !areas.parent.is_empty() {
        draw_parent(frame, app, areas.parent);
    }
    draw_files(frame, app, areas.files);
    if !areas.preview.is_empty() {
        draw_preview(frame, app, areas.preview);
    }
    draw_dock(frame, app, areas.dock);
    draw_status(frame, app, areas.status);
    draw_toasts(frame, app, areas.files.union(areas.preview));
    draw_overlay(frame, app);
}

/// Whether the long-name marquee needs frames.
pub fn marquee_active(app: &App) -> bool {
    app.selected().is_some_and(|entry| {
        entry.server_filename.width() > name_width(app, usize::from(app.regions.files.width), true)
    })
}

fn header(app: &App, area: Rect) -> (Line<'static>, Vec<(Rect, String)>) {
    let theme = app.theme;
    let mut spans = vec![Span::raw(" ")];
    let mut crumbs = Vec::new();
    let mut right = Vec::new();
    if app.is_loading() {
        right.push(Span::styled(
            theme.spinner(app.spinner_frame()),
            theme.accent(),
        ));
        right.push(Span::raw("  "));
    }
    if let Some(quota) = app.quota.filter(|quota| quota.total > 0) {
        right.push(Span::styled(theme.glyph(Glyph::Cloud), theme.dim()));
        right.push(Span::styled(
            format!(
                "{} / {} ",
                format::size(quota.used),
                format::size(quota.total)
            ),
            theme.dim(),
        ));
    }
    let right_width: usize = right.iter().map(Span::width).sum();
    let budget = usize::from(area.width).saturating_sub(right_width + 2);

    if let Some(search) = &app.search {
        spans.push(Span::styled(theme.glyph(Glyph::Search), theme.warning()));
        spans.push(Span::styled(
            format!("\"{}\"", search.keyword),
            theme.warning().bold(),
        ));
        spans.push(Span::styled(format!(" in {}", search.root), theme.dim()));
    } else {
        let mut segments = vec![(
            theme.glyph(Glyph::Root).trim_end().to_string(),
            "/".to_string(),
        )];
        let mut path = String::new();
        for part in app.cwd.split('/').filter(|part| !part.is_empty()) {
            path = remote_path::join(&path, part);
            segments.push((part.to_string(), path.clone()));
        }
        let separator = theme.glyph(Glyph::Separator);
        let total: usize = segments
            .iter()
            .map(|(label, _)| label.width() + separator.width())
            .sum();
        let mut skip = 0;
        let mut width = total;
        while width > budget && skip + 1 < segments.len() {
            width -= segments[skip].0.width() + separator.width();
            skip += 1;
        }
        if skip > 0 {
            spans.push(Span::styled("…", theme.dim()));
            spans.push(Span::styled(separator, theme.dim()));
        }
        let last = segments.len() - 1;
        for (index, (label, path)) in segments.into_iter().enumerate().skip(skip) {
            let x = area.x + spans.iter().map(Span::width).sum::<usize>() as u16;
            let label = if index == last {
                format::truncate(&label, budget.saturating_sub(x as usize)).into_owned()
            } else {
                label
            };
            crumbs.push((Rect::new(x, area.y, label.width() as u16, 1), path));
            let style = if index == last {
                theme.title()
            } else {
                theme.text()
            };
            spans.push(Span::styled(label, style));
            // In ASCII mode the root label is itself the first slash.
            if index != last && !(index == 0 && theme.icons == IconSet::Ascii) {
                spans.push(Span::styled(separator, theme.dim()));
            }
        }
    }
    let left_width: usize = spans.iter().map(Span::width).sum();
    let pad = usize::from(area.width).saturating_sub(left_width + right_width);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.extend(right);
    (Line::from(spans), crumbs)
}

fn name_width(app: &App, column_width: usize, with_size: bool) -> usize {
    let icon = app.theme.icon(FileKind::Other).width();
    let size = if with_size { SIZE_WIDTH + 2 } else { 1 };
    column_width.saturating_sub(1 + icon + size)
}

struct RowStyle {
    background: Option<Style>,
    size: bool,
    marked: bool,
    marquee: Option<usize>,
}

/// One file row: gutter, icon, name, right-aligned size.
fn entry_row(app: &App, entry: &FileEntry, width: usize, row: RowStyle) -> Line<'static> {
    let theme = app.theme;
    let kind = FileKind::of(entry);
    let mut name = entry.server_filename.clone();
    if entry.is_dir && theme.icon(kind).is_empty() {
        name.push('/');
    }
    let size = if !row.size {
        String::new()
    } else if entry.is_dir {
        app.cache
            .get(&entry.path)
            .filter(|listing| listing.is_loaded())
            .map(|listing| listing.entries.len().to_string())
            .unwrap_or_default()
    } else {
        format::size(entry.size_bytes())
    };
    let name_width = name_width(app, width, row.size);
    let name = match row.marquee {
        Some(offset) => format::fit(&format::window(&name, offset, name_width), name_width),
        None => format::fit(&name, name_width),
    };
    let name_style = if row.marked {
        theme.marked()
    } else {
        theme.kind(kind)
    };
    let gutter = if row.marked {
        Span::styled("▍", theme.marked())
    } else {
        Span::raw(" ")
    };
    let mut line = Line::from(vec![
        gutter,
        Span::styled(theme.icon(kind), theme.kind(kind)),
        Span::styled(name, name_style),
        Span::styled(
            if row.size {
                format!(" {size:>SIZE_WIDTH$} ")
            } else {
                " ".to_string()
            },
            theme.dim(),
        ),
    ]);
    if let Some(background) = row.background {
        line = line.patch_style(background);
        line.spans
            .iter_mut()
            .for_each(|span| span.style = span.style.patch(background));
    }
    line
}

fn marquee_offset(app: &App, name: &str, width: usize) -> Option<usize> {
    let overflow = name
        .width()
        .checked_sub(name_width(app, width, true))
        .filter(|o| *o > 0)?;
    let idle = app.cursor_moved_at.elapsed().checked_sub(MARQUEE_DELAY)?;
    // Scroll to the end, pause, then start over.
    let step = (idle.as_millis() / MARQUEE_STEP_MS) as usize % (overflow + 12);
    Some(step.min(overflow))
}

fn draw_files(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = app.theme;
    let entries = app.entries();
    let width = usize::from(area.width);
    if app.column.is_empty() {
        let message = empty_message(app);
        frame.render_widget(
            Paragraph::new(Line::styled(message.0, message.1)).centered(),
            Rect {
                y: area.y + area.height / 3,
                height: 1,
                ..area
            },
        );
        return;
    }
    let window = app.column.window();
    let lines: Vec<Line<'_>> = app
        .column
        .entries_in(window.clone())
        .iter()
        .zip(window.clone())
        .map(|(&index, position)| {
            let entry = &entries[index];
            let is_cursor = position == app.column.cursor();
            let background = is_cursor.then(|| match app.focus {
                Focus::Files => theme.cursor(),
                Focus::Preview => theme.cursor_inactive(),
            });
            let marquee = if is_cursor {
                marquee_offset(app, &entry.server_filename, width)
            } else {
                None
            };
            entry_row(
                app,
                entry,
                width,
                RowStyle {
                    background,
                    size: true,
                    marked: app.is_marked(position, &entry.path),
                    marquee,
                },
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
    if app.column.len() > usize::from(area.height) {
        let mut state =
            ScrollbarState::new(app.column.len().saturating_sub(usize::from(area.height)))
                .position(app.column.offset());
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(None)
                .thumb_symbol("▐")
                .thumb_style(theme.dim()),
            area,
            &mut state,
        );
    }
}

fn empty_message(app: &App) -> (String, Style) {
    let theme = app.theme;
    if !app.column.filter().is_empty() {
        return (
            format!("no match for “{}”", app.column.filter()),
            theme.dim(),
        );
    }
    if let Some(search) = &app.search {
        let text = if search.running {
            "searching…"
        } else {
            "no results"
        };
        return (text.to_string(), theme.dim());
    }
    match app.cache.get(&app.cwd) {
        Some(listing) if listing.error.is_some() && !listing.is_loaded() => {
            (listing.error.clone().unwrap_or_default(), theme.error())
        }
        Some(listing) if listing.is_loaded() => ("empty directory".to_string(), theme.dim()),
        _ => ("loading…".to_string(), theme.dim()),
    }
}

fn side_listing(frame: &mut Frame<'_>, app: &App, area: Rect, dir: &str, highlight: Option<&str>) {
    let theme = app.theme;
    let entries = app.cache.entries(dir);
    let width = usize::from(area.width);
    if entries.is_empty() {
        let (text, style) = match app.cache.get(dir) {
            Some(listing) if listing.is_loaded() => ("empty", theme.dim()),
            Some(listing) if listing.error.is_some() => ("cannot list directory", theme.error()),
            _ => ("loading…", theme.dim()),
        };
        frame.render_widget(
            Paragraph::new(Line::styled(format!(" {text}"), style)),
            area,
        );
        return;
    }
    let highlighted =
        highlight.and_then(|path| entries.iter().position(|entry| entry.path == path));
    let offset = super::app::side_offset(highlighted, entries.len(), usize::from(area.height));
    let lines: Vec<Line<'_>> = entries
        .iter()
        .enumerate()
        .skip(offset)
        .take(usize::from(area.height))
        .map(|(index, entry)| {
            entry_row(
                app,
                entry,
                width,
                RowStyle {
                    background: (Some(index) == highlighted).then(|| theme.cursor_inactive()),
                    size: width >= 32,
                    marked: app.marks.contains(&entry.path),
                    marquee: None,
                },
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_parent(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (dir, highlight) = match &app.search {
        Some(search) => (search.root.clone(), None),
        None => match remote_path::parent(&app.cwd) {
            Some(parent) => (parent, Some(app.cwd.as_str())),
            None => return,
        },
    };
    side_listing(frame, app, area, &dir, highlight);
}

fn draw_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = app.theme;
    let Some(entry) = app.selected() else {
        return;
    };
    let area = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(1),
        ..area
    };
    if entry.is_dir {
        side_listing(frame, app, area, &entry.path, None);
        return;
    }

    let kind = FileKind::of(entry);
    let label_style = theme.dim();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(theme.icon(kind), theme.kind(kind)),
            Span::styled(
                format::truncate(
                    &entry.server_filename,
                    usize::from(area.width).saturating_sub(2),
                )
                .into_owned(),
                theme.kind(kind).bold(),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Size      ", label_style),
            Span::raw(format!(
                "{}  ({} bytes)",
                format::size(entry.size_bytes()),
                entry.size_bytes()
            )),
        ]),
        Line::from(vec![
            Span::styled("Modified  ", label_style),
            Span::raw(format::local_time(entry.mtime)),
        ]),
        Line::from(vec![
            Span::styled("Kind      ", label_style),
            Span::raw(kind.label()),
        ]),
    ];
    if app.search.is_some() {
        lines.push(Line::from(vec![
            Span::styled("Folder    ", label_style),
            Span::raw(remote_path::parent(&entry.path).unwrap_or_default()),
        ]));
    }
    let header_height = (lines.len() as u16).min(area.height);
    frame.render_widget(
        Paragraph::new(lines),
        Rect {
            height: header_height,
            ..area
        },
    );

    let rule_y = area.y + header_height;
    if rule_y >= area.bottom() {
        return;
    }
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(area.width))).style(theme.border()),
        Rect {
            y: rule_y,
            height: 1,
            ..area
        },
    );
    let body = Rect {
        y: rule_y + 1,
        height: area.bottom().saturating_sub(rule_y + 1),
        ..area
    };
    if app.preview.path != entry.path {
        return;
    }
    let width = usize::from(body.width);
    let paragraph = match &app.preview.body {
        PreviewBody::Empty => return,
        PreviewBody::Loading => Paragraph::new(Line::styled(
            format!("{} loading preview…", theme.spinner(app.spinner_frame())),
            theme.dim(),
        )),
        PreviewBody::Text(text) => {
            let number_width = text.lines().count().to_string().len().max(2);
            let lines: Vec<Line<'_>> = text
                .lines()
                .enumerate()
                .skip(app.preview.scroll)
                .take(usize::from(body.height))
                .map(|(index, line)| {
                    let line = line.replace('\t', "    ");
                    Line::from(vec![
                        Span::styled(format!("{:>number_width$} ", index + 1), theme.dim()),
                        Span::raw(
                            format::truncate(&line, width.saturating_sub(number_width + 1))
                                .into_owned(),
                        ),
                    ])
                })
                .collect();
            Paragraph::new(lines)
        }
        PreviewBody::Image(description) => {
            Paragraph::new(Line::styled(description.clone(), theme.accent()))
        }
        PreviewBody::Note(note) => {
            Paragraph::new(Line::styled(note.clone(), theme.dim())).wrap(Wrap { trim: true })
        }
        PreviewBody::Error(error) => {
            Paragraph::new(Line::styled(error.clone(), theme.error())).wrap(Wrap { trim: true })
        }
    };
    frame.render_widget(paragraph, body);
    if app.focus == Focus::Preview && app.preview_line_count() > usize::from(body.height) {
        let mut state = ScrollbarState::new(
            app.preview_line_count()
                .saturating_sub(usize::from(body.height)),
        )
        .position(app.preview.scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(None)
                .thumb_symbol("▐")
                .thumb_style(theme.accent()),
            body,
            &mut state,
        );
    }
}

fn progress_bar(theme: Theme, fraction: f64, width: usize) -> Vec<Span<'static>> {
    let halves = (fraction * (width * 2) as f64).round() as usize;
    let full = halves / 2;
    let half = halves % 2 == 1;
    let rest = width.saturating_sub(full + usize::from(half));
    let mut spans = vec![Span::styled("━".repeat(full), theme.accent())];
    if half {
        spans.push(Span::styled("╸", theme.accent()));
    }
    spans.push(Span::styled("─".repeat(rest), theme.dim()));
    spans
}

fn task_line(app: &App, task: &Task, width: usize) -> Line<'static> {
    let theme = app.theme;
    let glyph = match task.kind {
        TaskKind::Download => Glyph::Download,
        TaskKind::Open => Glyph::Open,
        TaskKind::Remote => Glyph::Remote,
        TaskKind::Search => Glyph::Search,
    };
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(theme.glyph(glyph), theme.accent()),
    ];
    let mut tail: Vec<Span<'static>> = Vec::new();
    match (&task.state, task.fraction()) {
        (TaskState::Cancelling, _) => tail.push(Span::styled("cancelling…", theme.warning())),
        (_, Some(fraction)) => {
            let bar = (width / 4).clamp(10, 30);
            tail.extend(progress_bar(theme, fraction, bar));
            tail.push(Span::raw(format!(" {:>3.0}%", fraction * 100.0)));
            let speed = if task.speed >= 1.0 {
                format!("{}/s", format::size(task.speed as u64))
            } else {
                String::new()
            };
            tail.push(Span::styled(format!("  {speed:>11}"), theme.dim()));
            let eta = task.eta().map(format::duration).unwrap_or_default();
            tail.push(Span::styled(format!("  {eta:>7} "), theme.dim()));
        }
        (_, None) => tail.push(Span::styled(
            format!("{} working ", theme.spinner(app.spinner_frame())),
            theme.dim(),
        )),
    }
    let used: usize = spans.iter().chain(&tail).map(Span::width).sum();
    let label_width = width.saturating_sub(used + 2);
    spans.push(Span::raw(format::fit(&task.label, label_width)));
    spans.push(Span::raw("  "));
    spans.extend(tail);
    Line::from(spans)
}

fn draw_dock(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }
    let width = usize::from(area.width);
    let lines: Vec<Line<'_>> = app
        .tasks
        .active()
        .rev()
        .take(usize::from(area.height))
        .map(|task| task_line(app, task, width))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = app.theme;
    if let Some(input) = &app.input {
        let badge = format!(" {} ", input.kind.label());
        let prompt = match input.kind {
            InputKind::Filter => " /",
            InputKind::Command => " :",
            _ => " ›",
        };
        let prefix_width = badge.width() + prompt.width() + 1;
        let line = Line::from(vec![
            Span::styled(badge, theme.badge(Color::Blue)),
            Span::styled(prompt, theme.accent()),
            Span::raw(" "),
            Span::raw(input.edit.text().to_string()),
        ]);
        frame.render_widget(line, area);
        let x = area.x + (prefix_width + input.edit.cursor_column()) as u16;
        frame.set_cursor_position(Position::new(x.min(area.right().saturating_sub(1)), area.y));
        return;
    }

    let (mode, color) = if app.visual.is_some() {
        ("VISUAL", Color::Magenta)
    } else if app.search.is_some() {
        ("SEARCH", Color::Yellow)
    } else if app.focus == Focus::Preview {
        ("PREVIEW", Color::Green)
    } else {
        ("NORMAL", Color::Cyan)
    };
    let mut left = vec![
        Span::styled(format!(" {mode} "), theme.badge(color)),
        Span::raw(" "),
    ];
    let segment = |spans: &mut Vec<Span<'static>>, text: String, style: Style| {
        spans.push(Span::styled(text, style));
        spans.push(Span::styled("  ", theme.dim()));
    };
    if !app.column.is_empty() {
        segment(
            &mut left,
            format!("{}/{}", app.column.cursor() + 1, app.column.len()),
            theme.text(),
        );
    }
    let marked = app.marked_count();
    if marked > 0 {
        segment(&mut left, format!("● {marked} marked"), theme.marked());
    }
    if let Some(clipboard) = &app.clipboard {
        let verb = if clipboard.cut { "cut" } else { "yanked" };
        segment(
            &mut left,
            format!("{} {verb}", clipboard.paths.len()),
            theme.accent(),
        );
    }
    if !app.column.filter().is_empty() {
        segment(
            &mut left,
            format!("/{}", app.column.filter()),
            theme.warning(),
        );
    }
    let sort = app.cache.sort();
    let arrow = if sort.ascending { "↑" } else { "↓" };
    segment(
        &mut left,
        format!("{}{arrow}", sort.key.label()),
        theme.dim(),
    );

    let right = vec![
        Span::styled("?", theme.key()),
        Span::styled(" help  ", theme.dim()),
        Span::styled("q", theme.key()),
        Span::styled(" quit ", theme.dim()),
    ];
    let used: usize = left.iter().chain(&right).map(Span::width).sum();
    left.push(Span::raw(
        " ".repeat(usize::from(area.width).saturating_sub(used)),
    ));
    left.extend(right);
    frame.render_widget(Line::from(left), area);
}

fn draw_toasts(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = app.theme;
    let mut bottom = area.bottom();
    for toast in app.toasts.iter().rev() {
        let (icon, style) = match toast.level {
            ToastLevel::Info => ("ℹ", theme.accent()),
            ToastLevel::Success => ("✓", theme.success()),
            ToastLevel::Error => ("✗", theme.error()),
        };
        let max_width = usize::from(area.width).saturating_sub(4).min(64);
        let text = format::truncate(&toast.text, max_width.saturating_sub(4));
        let width = (text.width() + 6) as u16;
        if bottom < area.y + 3 || width > area.width {
            break;
        }
        let rect = Rect::new(area.right().saturating_sub(width + 1), bottom - 3, width, 3);
        bottom -= 3;
        clear_cut_wide_chars(frame, rect);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{icon} "), style.bold()),
                Span::raw(text.into_owned()),
            ]))
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(style)
                    .padding(ratatui::widgets::Padding::horizontal(1)),
            ),
            rect,
        );
    }
}

/// Dim everything behind a dialog, then draw an empty rounded box.
fn popup(frame: &mut Frame<'_>, theme: Theme, title: &str, width: u16, height: u16) -> Rect {
    let screen = frame.area();
    frame
        .buffer_mut()
        .set_style(screen, Style::new().add_modifier(Modifier::DIM));
    let width = width.min(screen.width.saturating_sub(4));
    let height = height.min(screen.height.saturating_sub(2));
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 3,
        width,
        height,
    );
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.accent())
        .title(Span::styled(format!(" {title} "), theme.title()))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner = block.inner(area);
    clear_cut_wide_chars(frame, area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    inner
}

/// A wide character just left of `area` would hide the box's left border
/// (terminals draw both its cells), so blank it before overlaying.
fn clear_cut_wide_chars(frame: &mut Frame<'_>, area: Rect) {
    let Some(x) = area.x.checked_sub(1) else {
        return;
    };
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        let cell = &mut buffer[(x, y)];
        if cell.symbol().width() > 1 {
            cell.set_symbol(" ");
        }
    }
}

fn key_hint(theme: Theme, keys: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (key, label) in keys {
        spans.push(Span::styled(*key, theme.key()));
        spans.push(Span::styled(format!(" {label}   "), theme.dim()));
    }
    Line::from(spans).centered()
}

fn draw_overlay(frame: &mut Frame<'_>, app: &App) {
    let theme = app.theme;
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => {
            let column_width = 34;
            let mut columns: [Vec<Line<'_>>; 2] = [Vec::new(), Vec::new()];
            for (index, (group, keys)) in HELP.iter().enumerate() {
                let lines = &mut columns[index % 2];
                lines.push(Line::styled(*group, theme.title()));
                for (key, description) in *keys {
                    lines.push(Line::from(vec![
                        Span::styled(format::fit(key, 9), theme.key()),
                        Span::raw(*description),
                    ]));
                }
                lines.push(Line::raw(""));
            }
            let height = columns.iter().map(Vec::len).max().unwrap_or(0) as u16 + 2;
            let inner = popup(frame, theme, "Keys", column_width * 2 + 6, height + 1);
            let [left, right] =
                Layout::horizontal([Constraint::Length(column_width + 2), Constraint::Fill(1)])
                    .areas(inner);
            let [left_lines, right_lines] = columns;
            frame.render_widget(Paragraph::new(left_lines), left);
            frame.render_widget(Paragraph::new(right_lines), right);
        }
        Overlay::Sort => {
            let current = app.cache.sort();
            let lines: Vec<Line<'_>> = SortKey::ALL
                .into_iter()
                .map(|key| {
                    let active = key == current.key;
                    let marker = match (active, current.ascending) {
                        (false, _) => "  ",
                        (true, true) => " ↑",
                        (true, false) => " ↓",
                    };
                    Line::from(vec![
                        Span::styled(format!("{}  ", key.hotkey()), theme.key()),
                        Span::styled(
                            format::fit(key.label(), 10),
                            if active { theme.title() } else { theme.text() },
                        ),
                        Span::styled(marker, theme.accent()),
                    ])
                })
                .collect();
            let inner = popup(frame, theme, "Sort by", 24, 6);
            frame.render_widget(Paragraph::new(lines), inner);
        }
        Overlay::Tasks { cursor } => {
            let height = (app.tasks.len() as u16).clamp(1, 16) + 3;
            let inner = popup(frame, theme, "Tasks", 90, height);
            let [list, hint] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
            let width = usize::from(list.width);
            let skip = cursor.saturating_sub(usize::from(list.height).saturating_sub(1));
            let lines: Vec<Line<'_>> = if app.tasks.is_empty() {
                vec![Line::styled("no tasks yet", theme.dim())]
            } else {
                app.tasks
                    .iter()
                    .rev()
                    .enumerate()
                    .skip(skip)
                    .map(|(index, task)| {
                        let line = if task.is_active() {
                            task_line(app, task, width)
                        } else {
                            finished_task_line(theme, task, width)
                        };
                        if index == *cursor {
                            line.patch_style(theme.cursor_inactive())
                        } else {
                            line
                        }
                    })
                    .collect()
            };
            frame.render_widget(Paragraph::new(lines), list);
            frame.render_widget(
                key_hint(theme, &[("j/k", "move"), ("c", "cancel"), ("Esc", "close")]),
                hint,
            );
        }
        Overlay::Confirm(operation) => {
            let details = operation.details();
            let shown = details.len().min(8);
            let mut lines: Vec<Line<'_>> = details
                .iter()
                .take(shown)
                .map(|detail| Line::styled(format!("  {detail}"), theme.text()))
                .collect();
            if details.len() > shown {
                lines.push(Line::styled(
                    format!("  … and {} more", details.len() - shown),
                    theme.dim(),
                ));
            }
            lines.push(Line::raw(""));
            lines.push(key_hint(
                theme,
                &[("y", operation.label()), ("n", "cancel")],
            ));
            let title = format!("{}?", capitalize(&operation.summary()));
            let width = details
                .iter()
                .map(|detail| detail.width() as u16 + 6)
                .max()
                .unwrap_or(30)
                .max(40);
            let inner = popup(frame, theme, &title, width, lines.len() as u16 + 2);
            frame.render_widget(Paragraph::new(lines), inner);
        }
        Overlay::ConfirmQuit => {
            let lines = vec![
                Line::raw("Transfers are still running."),
                Line::styled("Partial downloads resume next time.", theme.dim()),
                Line::raw(""),
                key_hint(theme, &[("y", "quit"), ("n", "stay")]),
            ];
            let inner = popup(frame, theme, "Quit?", 44, 6);
            frame.render_widget(Paragraph::new(lines), inner);
        }
    }
}

fn finished_task_line(theme: Theme, task: &Task, width: usize) -> Line<'static> {
    let (icon, style, note) = match &task.state {
        TaskState::Done => ("✓", theme.success(), String::new()),
        TaskState::Failed(error) => ("✗", theme.error(), error.clone()),
        TaskState::Cancelled => ("⊘", theme.dim(), "cancelled".to_string()),
        TaskState::Running | TaskState::Cancelling => ("…", theme.dim(), String::new()),
    };
    let label_width = (width / 2).max(10);
    Line::from(vec![
        Span::styled(format!(" {icon} "), style),
        Span::raw(format::fit(&task.label, label_width)),
        Span::styled(
            format!(
                " {}",
                format::truncate(&note, width.saturating_sub(label_width + 4))
            ),
            style,
        ),
    ])
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;
    use crate::app::ops::Operation;
    use crate::app::task::TaskKind;
    use crate::config::IconSet;
    use crate::tui::app::Overlay;
    use crate::tui::theme::Theme;

    fn entry(path: &str, is_dir: bool, size: i64) -> FileEntry {
        FileEntry {
            fs_id: 1,
            path: path.to_string(),
            server_filename: remote_path::file_name(path).to_string(),
            size,
            mtime: 1_790_000_000,
            is_dir,
        }
    }

    fn load(app: &mut App, path: &str, entries: Vec<FileEntry>) {
        let generation = app.cache.begin_refresh(path);
        app.cache.finish_refresh(path, generation, Ok(entries));
        app.listing_updated(path);
    }

    fn sample(icons: IconSet) -> App {
        let root = "/我的资源/L 绿灯军团（2026）";
        let mut app = App::new(root, Theme::new(icons));
        load(
            &mut app,
            "/我的资源",
            vec![
                entry("/我的资源/电影", true, 0),
                entry(root, true, 0),
                entry("/我的资源/音乐", true, 0),
            ],
        );
        load(&mut app, root, vec![
            entry(&format!("{root}/1080P版"), true, 0),
            entry(&format!("{root}/Lanterns.S01E01.2026.2160p.AMZN.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265.mkv"), false, 8_390_226_165),
            entry(&format!("{root}/Lanterns.S01E02.mkv"), false, 5_927_419_042),
            entry(&format!("{root}/zh.ass"), false, 51_234),
        ]);
        app.enter_dir(root);
        app
    }

    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                let mut line = String::new();
                let mut skip = 0;
                for x in 0..width {
                    let symbol = buffer[(x, y)].symbol();
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    skip = symbol.width().saturating_sub(1);
                    line.push_str(symbol);
                }
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn three_columns_when_wide() {
        let mut app = sample(IconSet::Ascii);
        app.handle_key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Char('j'),
        ));
        let screen = render(&mut app, 120, 12);
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn two_columns_with_dock_and_marks() {
        let mut app = sample(IconSet::Ascii);
        app.marks
            .insert("/我的资源/L 绿灯军团（2026）/zh.ass".to_string());
        let id = app
            .tasks
            .start(TaskKind::Download, "Lanterns.S01E02.mkv", Some(1000));
        app.tasks.progress(id, 580);
        let screen = render(&mut app, 80, 10);
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn single_column_when_narrow() {
        let mut app = sample(IconSet::Ascii);
        let screen = render(&mut app, 50, 8);
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn confirm_dialog() {
        let mut app = sample(IconSet::Ascii);
        app.overlay = Overlay::Confirm(Operation::Delete(vec!["/a/one".into(), "/a/two".into()]));
        let screen = render(&mut app, 80, 14);
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn help_overlay_fits() {
        let mut app = sample(IconSet::Nerd);
        app.overlay = Overlay::Help;
        let screen = render(&mut app, 100, 30);
        insta::assert_snapshot!(screen);
    }
}
