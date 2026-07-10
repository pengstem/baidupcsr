use super::*;

pub(crate) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub(crate) fn top_centered_rect(area: Rect, width: u16, height: u16, y_offset: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + y_offset.min(area.height.saturating_sub(height)),
        width,
        height,
    }
}

pub(crate) fn surface_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_BG)
}

pub(crate) fn content_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_PANEL)
}

pub(crate) fn meta_value_style() -> Style {
    Style::new().fg(THEME_TEXT).bg(THEME_PANEL)
}

pub(crate) fn confirm_style() -> Style {
    Style::new()
        .fg(THEME_ERROR)
        .bg(THEME_PANEL)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn title_style(active: bool) -> Style {
    let color = if active {
        THEME_ACCENT
    } else {
        THEME_DIRECTORY
    };
    Style::new()
        .fg(color)
        .bg(THEME_BG)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn themed_block(title: impl Into<String>, active: bool) -> Block<'static> {
    let border = if active {
        THEME_BORDER_ACTIVE
    } else {
        THEME_BORDER
    };
    Block::bordered()
        .style(surface_style())
        .border_style(Style::new().fg(border).bg(THEME_BG))
        .title(Line::from(Span::styled(title.into(), title_style(active))))
}

pub(crate) fn detail_value_width(inner_width: usize, label: &str) -> usize {
    inner_width
        .saturating_sub(UnicodeWidthStr::width(label))
        .saturating_sub(1)
        .max(1)
}

pub(crate) fn detail_line(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(label, Style::new().fg(THEME_MUTED).bg(THEME_PANEL)),
        Span::styled(" ", surface_style()),
        Span::styled(value, meta_value_style()),
    ])
}

pub(crate) fn content_line(value: impl Into<String>) -> Line<'static> {
    Line::raw(value.into()).style(content_style())
}

pub(crate) fn help_section_line(value: &'static str) -> Line<'static> {
    Line::raw(value).style(
        Style::new()
            .fg(THEME_ACCENT)
            .bg(THEME_PANEL)
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn help_widget() -> Paragraph<'static> {
    Paragraph::new(vec![
        help_section_line("Navigation"),
        content_line("  j/k Up/Down        move selection"),
        content_line("  gg/G Home/End      first / last"),
        content_line("  PgUp/PgDn C-u/C-d  page / half-page"),
        content_line("  h/l Left/Right     parent / enter"),
        content_line("  Tab                switch pane focus"),
        content_line("  Enter              submit input"),
        Line::raw("").style(surface_style()),
        help_section_line("Actions"),
        content_line("  f filter  s search : command"),
        content_line("  ? help             Esc close/cancel"),
        content_line("  space mark         u refresh"),
        content_line("  x delete           r rename"),
        content_line("  , sort             c cancel task"),
        content_line("  d download         p preview/open  q quit"),
        content_line("  y/n                confirm / cancel"),
        Line::raw("").style(surface_style()),
        help_section_line("Mouse"),
        content_line("  file click selects double-click opens"),
        content_line("  wheel follows mouse pane  path click jumps"),
    ])
    .block(themed_block("Shortcuts", true))
    .style(surface_style())
    .wrap(Wrap { trim: true })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BreadcrumbSegment {
    label: String,
    target: String,
}

pub(crate) fn breadcrumb_segments(path: &str) -> Vec<BreadcrumbSegment> {
    let normalized = normalize_path(path);
    let mut segments = vec![BreadcrumbSegment {
        label: "/".to_string(),
        target: "/".to_string(),
    }];
    let mut target = String::new();
    for part in normalized
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
    {
        target.push('/');
        target.push_str(part);
        segments.push(BreadcrumbSegment {
            label: part.to_string(),
            target: target.clone(),
        });
    }
    segments
}

pub(crate) fn breadcrumb_spans(path: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, segment) in breadcrumb_segments(path).into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                BREADCRUMB_SEPARATOR,
                Style::new().fg(THEME_DIM).bg(THEME_BG),
            ));
        }
        spans.push(Span::styled(
            format!(" {} ", segment.label),
            Style::new()
                .fg(THEME_TEXT)
                .bg(THEME_MARKED)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

pub(crate) fn breadcrumb_path_at(
    path: &str,
    header: Rect,
    column: u16,
    row: u16,
) -> Option<String> {
    let inner = inner_rect(header);
    if row != inner.y || column < inner.x || column >= inner.right() {
        return None;
    }

    let mut cursor = inner
        .x
        .saturating_add(text_width_u16(HEADER_BRAND))
        .saturating_add(text_width_u16(HEADER_GAP));
    for (index, segment) in breadcrumb_segments(path).into_iter().enumerate() {
        if index > 0 {
            cursor = cursor.saturating_add(text_width_u16(BREADCRUMB_SEPARATOR));
        }
        let label_width = text_width_u16(segment.label.as_str()).saturating_add(2);
        let end = cursor.saturating_add(label_width);
        if column >= cursor && column < end {
            return Some(segment.target);
        }
        cursor = end;
    }
    None
}

pub(crate) fn text_width_u16(value: &str) -> u16 {
    UnicodeWidthStr::width(value).min(usize::from(u16::MAX)) as u16
}

pub(crate) fn header_widget(app: &TuiApp) -> Paragraph<'_> {
    let mut spans = vec![
        Span::styled(
            HEADER_BRAND,
            Style::new()
                .fg(THEME_ACCENT)
                .bg(THEME_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(HEADER_GAP, surface_style()),
    ];
    spans.extend(breadcrumb_spans(app.path()));
    spans.extend([
        Span::styled(HEADER_GAP, surface_style()),
        Span::styled(
            app.request_state.user_label(),
            Style::new()
                .fg(request_state_color(app.request_state))
                .bg(THEME_BG)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    Paragraph::new(Line::from(spans))
        .block(themed_block("Session", false))
        .style(surface_style())
}

pub(crate) fn render_files_pane(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let rows = files_visible_rows(area);
    let visible_indices = app.filtered_indices();
    let range = app.visible_entry_range(rows);
    let selected_row = app.selected_index().and_then(|selected| {
        visible_indices[range.clone()]
            .iter()
            .position(|index| *index == selected)
    });
    let mut table_state = TableState::new().with_selected(selected_row);
    frame.render_stateful_widget(files_widget(app, area), area, &mut table_state);

    let total = app.filtered_entry_count();
    if total <= rows || rows == 0 {
        return;
    }

    let range = app.visible_entry_range(rows);
    let mut state = ScrollbarState::new(total)
        .position(scrollbar_position_for_offset(total, rows, range.start))
        .viewport_content_length(rows);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(Style::new().fg(THEME_ACCENT).bg(THEME_PANEL))
            .track_style(Style::new().fg(THEME_BORDER).bg(THEME_PANEL)),
        inner_rect(area),
        &mut state,
    );
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileTableColumns {
    widths: Vec<Constraint>,
    name_width: usize,
    show_mtime: bool,
}

pub(crate) fn files_visible_rows(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(2))
}

pub(crate) fn file_table_columns(inner_width: usize) -> FileTableColumns {
    let show_mtime = inner_width >= 64;
    let size_width = 11;
    let mtime_width = 16;
    let fixed_width = if show_mtime {
        1 + 3 + size_width + mtime_width
    } else {
        1 + 3 + size_width
    };
    let column_count: usize = if show_mtime { 5 } else { 4 };
    let spacing_width = column_count.saturating_sub(1);
    let name_width = inner_width
        .saturating_sub(fixed_width)
        .saturating_sub(spacing_width)
        .max(4);
    let mut widths = vec![
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(name_width as u16),
        Constraint::Length(size_width as u16),
    ];
    if show_mtime {
        widths.push(Constraint::Length(mtime_width as u16));
    }
    FileTableColumns {
        widths,
        name_width,
        show_mtime,
    }
}

pub(crate) fn files_widget(app: &TuiApp, area: Rect) -> Table<'_> {
    let rows = files_visible_rows(area);
    let visible_indices = app.filtered_indices();
    let range = app.visible_entry_range(rows);
    let inner_width = area.width.saturating_sub(2) as usize;
    let columns = file_table_columns(inner_width);
    let items: Vec<Row<'_>> = if range.is_empty() {
        vec![Row::new(vec![Cell::from(
            Line::raw(files_empty_message(app)).style(meta_value_style()),
        )])]
    } else {
        visible_indices[range.clone()]
            .iter()
            .map(|index| {
                let entry = &app.entries()[*index];
                let selected = app.selected_index() == Some(*index);
                let marked = app.selected_paths.contains(&entry.path);
                let cursor_marker = if selected { ">" } else { " " };
                let mark_marker = if marked { "[x]" } else { "[ ]" };
                let suffix = if entry.is_dir { "/" } else { "" };
                let size = file_size_label(entry);
                let row_bg = if selected {
                    THEME_SELECTED
                } else if marked {
                    THEME_MARKED
                } else {
                    THEME_PANEL
                };
                let name_width = columns.name_width;
                let name_value = format!("{}{}", entry.server_filename, suffix);
                let name = if selected {
                    marquee_cells(name_value.as_str(), name_width, app.marquee_tick)
                } else {
                    truncate_cells(name_value.as_str(), name_width)
                };
                let base_style = if selected {
                    Style::new()
                        .fg(THEME_TEXT)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(THEME_TEXT).bg(row_bg)
                };
                let name_style = if selected {
                    base_style
                } else if entry.is_dir {
                    Style::new()
                        .fg(THEME_DIRECTORY)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(THEME_TEXT).bg(row_bg)
                };
                let cursor_style = if selected {
                    Style::new()
                        .fg(THEME_ACCENT)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(THEME_DIM).bg(row_bg)
                };
                let mark_style = if marked {
                    Style::new()
                        .fg(THEME_WARNING)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(THEME_DIM).bg(row_bg)
                };
                let size_style = if selected {
                    Style::new().fg(THEME_TEXT).bg(row_bg)
                } else {
                    Style::new().fg(THEME_DIM).bg(row_bg)
                };
                let mtime_style = if selected {
                    Style::new().fg(THEME_TEXT).bg(row_bg)
                } else {
                    Style::new().fg(THEME_DIM).bg(row_bg)
                };
                let mut cells = vec![
                    Cell::from(Line::from(Span::styled(cursor_marker, cursor_style))),
                    Cell::from(Line::from(Span::styled(mark_marker, mark_style))),
                    Cell::from(Line::from(Span::styled(name, name_style))),
                    Cell::from(Line::from(Span::styled(size, size_style))),
                ];
                if columns.show_mtime {
                    cells.push(Cell::from(Line::from(Span::styled(
                        modified_time_label(entry.mtime),
                        mtime_style,
                    ))));
                }
                Row::new(cells).style(base_style)
            })
            .collect()
    };

    let total = visible_indices.len();
    let title = if let Some(state) = app.search_results_state() {
        let prefix = if state.running { "Search" } else { "Results" };
        if total == 0 {
            format!("{prefix} 0 matches")
        } else if app.filter_input.is_empty() {
            format!(
                "{prefix} {}-{}/{} matches",
                range.start + 1,
                range.end,
                total
            )
        } else {
            format!(
                "{prefix} {}-{}/{} of {} matches",
                range.start + 1,
                range.end,
                total,
                app.entries().len()
            )
        }
    } else if total == 0 {
        if app.filter_input.is_empty() {
            "Files 0 items".to_string()
        } else {
            format!("Files 0/{} items", app.entries().len())
        }
    } else if app.filter_input.is_empty() {
        format!("Files {}-{}/{}", range.start + 1, range.end, total)
    } else {
        format!(
            "Files {}-{}/{} of {}",
            range.start + 1,
            range.end,
            total,
            app.entries().len()
        )
    };
    let widths = if range.is_empty() {
        vec![Constraint::Min(1)]
    } else {
        columns.widths
    };
    Table::new(items, widths)
        .block(themed_block(title, app.focus == PanelFocus::Files))
        .style(surface_style())
        .column_spacing(1)
        .highlight_spacing(HighlightSpacing::Never)
        .row_highlight_style(
            Style::new()
                .fg(THEME_TEXT)
                .bg(THEME_SELECTED)
                .add_modifier(Modifier::BOLD),
        )
}

pub(crate) fn preview_widget(
    app: &TuiApp,
    preview: Option<PreviewBudget>,
    area: Rect,
) -> Paragraph<'_> {
    let inner = inner_rect(area);
    let inner_width = usize::from(inner.width);
    let selected = app.selected_entry();
    let title = if app.preview_title.is_empty() {
        selected
            .map(|entry| entry.server_filename.as_str())
            .or(app.notice.as_deref())
            .unwrap_or("metadata")
    } else {
        app.preview_title.as_str()
    };
    let budget = preview.unwrap_or(app.preview_budget);

    let mut lines = selected.map_or_else(
        || {
            let name_width = detail_value_width(inner_width, "Name:");
            vec![
                detail_line("Name:", marquee_cells(title, name_width, app.marquee_tick)),
                detail_line("Kind:", "none".to_string()),
                detail_line("Cache:", app.cache_source.user_label().to_string()),
            ]
        },
        |entry| {
            preview_detail_lines(
                entry,
                app.cache_source,
                budget,
                inner_width,
                app.marquee_tick,
            )
        },
    );
    lines.push(Line::raw("").style(surface_style()));
    if budget.kind == PreviewKind::Video && !app.preview_lines.is_empty() {
        push_plain_lines(&mut lines, &app.preview_lines, "", content_style());
        lines.push(Line::raw("").style(surface_style()));
        lines.push(detail_line(
            "Buffer:",
            format!(
                "{} / {}",
                bytes_label(budget.fetched_bytes),
                bytes_label(budget.memory_limit_bytes)
            ),
        ));
    } else if budget.kind == PreviewKind::Text && !app.preview_lines.is_empty() {
        lines.truncate(4);
        lines.push(detail_line(
            "Preview:",
            format!("text · {} fetched", bytes_label(budget.fetched_bytes)),
        ));
        lines.push(Line::raw("").style(surface_style()));
        push_plain_lines(&mut lines, &app.preview_lines, "", content_style());
    } else {
        lines.push(detail_line("Preview:", budget.kind.label().to_string()));
        lines.push(Line::raw("").style(surface_style()));
        push_plain_lines(&mut lines, &app.preview_lines, "", content_style());
    }

    let scroll = app
        .preview_scroll_offset
        .min(lines.len().saturating_sub(usize::from(inner.height)))
        .min(usize::from(u16::MAX)) as u16;
    Paragraph::new(lines)
        .block(themed_block("Preview", app.focus == PanelFocus::Preview))
        .style(surface_style())
        .scroll((scroll, 0))
        .wrap(Wrap { trim: true })
}

pub(crate) fn render_preview_pane(
    frame: &mut Frame<'_>,
    app: &TuiApp,
    preview: Option<PreviewBudget>,
    area: Rect,
) {
    frame.render_widget(preview_widget(app, preview, area), area);

    let rows = usize::from(inner_rect(area).height);
    let total = app.preview_render_line_count();
    if rows == 0 || total <= rows {
        return;
    }

    let mut state = ScrollbarState::new(total)
        .position(scrollbar_position_for_offset(
            total,
            rows,
            app.preview_scroll_offset,
        ))
        .viewport_content_length(rows);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(Style::new().fg(THEME_ACCENT).bg(THEME_PANEL))
            .track_style(Style::new().fg(THEME_BORDER).bg(THEME_PANEL)),
        inner_rect(area),
        &mut state,
    );
}

pub(crate) fn preview_detail_lines(
    entry: &FileEntry,
    source: CacheSource,
    budget: PreviewBudget,
    inner_width: usize,
    marquee_tick: u64,
) -> Vec<Line<'static>> {
    let item_type = if entry.is_dir { "directory" } else { "file" };
    let action = if entry.is_dir {
        "l enter directory"
    } else if budget.kind == PreviewKind::Video {
        "p preview video"
    } else {
        "p open local copy (<100MiB)"
    };
    let name_width = detail_value_width(inner_width, "Name:");
    let path_width = detail_value_width(inner_width, "Path:");
    vec![
        detail_line(
            "Name:",
            marquee_cells(entry.server_filename.as_str(), name_width, marquee_tick),
        ),
        detail_line("Kind:", format!("{item_type} · {}", budget.kind.label())),
        detail_line("Size:", file_size_label(entry)),
        detail_line("Modified:", modified_time_label(entry.mtime)),
        detail_line("Cache:", source.user_label().to_string()),
        detail_line("ID:", entry.fs_id.to_string()),
        detail_line(
            "Path:",
            marquee_cells(entry.path.as_str(), path_width, marquee_tick),
        ),
        detail_line("Action:", action.to_string()),
    ]
}

pub(crate) fn default_preview_budget() -> PreviewBudget {
    PreviewBudget {
        kind: PreviewKind::Metadata,
        fetched_bytes: 0,
        memory_limit_bytes: 16 * 1024 * 1024,
    }
}

pub(crate) fn push_limited_lines(
    output: &mut Vec<Line<'static>>,
    source: &[String],
    limit: usize,
    prefix: &str,
    style: Style,
) {
    if limit == 0 {
        return;
    }
    let shown = if source.len() > limit {
        limit.saturating_sub(1)
    } else {
        limit
    };
    for line in source.iter().take(shown) {
        output.push(Line::raw(format!("{prefix}{line}")).style(style));
    }
    if source.len() > limit {
        output.push(Line::raw(format!("+{} more", source.len() - shown)).style(meta_value_style()));
    }
}

pub(crate) fn push_plain_lines(
    output: &mut Vec<Line<'static>>,
    source: &[String],
    prefix: &str,
    style: Style,
) {
    output.extend(
        source
            .iter()
            .map(|line| Line::raw(format!("{prefix}{line}")).style(style)),
    );
}

pub(crate) fn render_tasks_pane(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    if app.task_items.is_empty() {
        frame.render_widget(tasks_widget(app), area);
        return;
    }

    frame.render_widget(themed_block("Tasks", false), area);
    let inner = inner_rect(area);
    if inner.height == 0 {
        return;
    }

    frame.render_widget(task_table_widget(app, usize::from(inner.height)), inner);
}

pub(crate) fn render_transfer_pane(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let Some(task) = app.task_items.first() else {
        return;
    };
    if area.is_empty() {
        return;
    }

    frame.render_widget(themed_block("Transfer", false), area);
    let inner = inner_rect(area);
    if inner.height == 0 {
        return;
    }

    let [progress_area, speed_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .areas(inner);
    let speed_history = app
        .task_speed_history
        .get(&task.id)
        .map_or(&[][..], Vec::as_slice);
    render_task_progress(frame, task, progress_area);
    render_task_speed(frame, task, speed_history, speed_area);
}

pub(crate) fn task_table_widget(app: &TuiApp, rows: usize) -> Table<'_> {
    let table_rows = app.task_items.iter().take(rows).map(|task| {
        let state_style = match task.state.as_str() {
            "failed" | "cancelled" => Style::new().fg(THEME_ERROR).bg(THEME_PANEL),
            "done" => Style::new().fg(THEME_SUCCESS).bg(THEME_PANEL),
            "running" => Style::new().fg(THEME_ACCENT).bg(THEME_PANEL),
            _ => meta_value_style(),
        };
        Row::new(vec![
            Cell::from(Line::raw(format!("#{}", task.id)).style(meta_value_style())),
            Cell::from(Line::raw(task.kind.clone()).style(meta_value_style())),
            Cell::from(Line::raw(task.state.clone()).style(state_style)),
            Cell::from(Line::raw(task.label.clone()).style(content_style())),
        ])
        .style(content_style())
    });
    Table::new(
        table_rows,
        [
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Min(12),
        ],
    )
    .column_spacing(1)
    .highlight_spacing(HighlightSpacing::Never)
    .style(surface_style())
}

pub(crate) fn render_task_progress(frame: &mut Frame<'_>, task: &TaskPanelItem, area: Rect) {
    if area.is_empty() {
        return;
    }
    if let Some(total) = task.bytes_total.filter(|total| *total > 0) {
        let ratio = (task.bytes_done as f64 / total as f64).clamp(0.0, 1.0);
        let label = format!(
            "{}% {} / {}",
            (ratio * 100.0).round() as u64,
            bytes_label(task.bytes_done),
            bytes_label(total)
        );
        frame.render_widget(
            Gauge::default()
                .ratio(ratio)
                .label(label)
                .style(content_style())
                .gauge_style(Style::new().fg(THEME_ACCENT).bg(THEME_MARKED)),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(Line::raw(format!(
                "progress {} / ?",
                bytes_label(task.bytes_done)
            )))
            .style(content_style()),
            area,
        );
    }
}

pub(crate) fn render_task_speed(
    frame: &mut Frame<'_>,
    task: &TaskPanelItem,
    history: &[u64],
    area: Rect,
) {
    if area.is_empty() {
        return;
    }
    let [label_area, spark_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(18), Constraint::Min(1)])
        .areas(area);
    frame.render_widget(
        Paragraph::new(Line::raw(format!(
            "{}/s",
            bytes_label(task.speed_bytes_per_sec)
        )))
        .style(meta_value_style()),
        label_area,
    );
    if history.is_empty() {
        return;
    }
    frame.render_widget(
        Sparkline::default()
            .data(history.iter().copied())
            .style(Style::new().fg(THEME_SUCCESS).bg(THEME_PANEL)),
        spark_area,
    );
}

pub(crate) fn tasks_widget(app: &TuiApp) -> Paragraph<'_> {
    let mut lines = Vec::new();
    if let Some(line) = task_status_line(app) {
        lines.push(line);
    }
    push_limited_lines(
        &mut lines,
        &app.preload_lines,
        2,
        "preload ",
        Style::new().fg(THEME_DIRECTORY).bg(THEME_PANEL),
    );
    if let Some(notice) = app.notice.as_deref() {
        lines.push(
            Line::raw(notice.to_string()).style(Style::new().fg(THEME_WARNING).bg(THEME_PANEL)),
        );
    }
    if lines.is_empty() {
        lines.push(Line::raw("No active tasks").style(meta_value_style()));
    }
    Paragraph::new(lines)
        .block(themed_block("Tasks", false))
        .style(surface_style())
}

pub(crate) fn confirm_widget(app: &TuiApp) -> Paragraph<'_> {
    let mut lines = Vec::new();
    let body_lines = confirm_body_lines(app);
    for (index, line) in body_lines.iter().take(9).enumerate() {
        let style = if index == 0 {
            confirm_style()
        } else {
            content_style()
        };
        lines.push(Line::raw(line.clone()).style(style));
    }
    if body_lines.len() > 9 {
        lines.push(Line::raw(format!("+{} more", body_lines.len() - 9)).style(meta_value_style()));
    }
    lines.push(Line::raw("").style(surface_style()));
    lines.push(Line::from(vec![
        Span::styled("[y] confirm", confirm_style()),
        Span::styled("    ", surface_style()),
        Span::styled(
            "[n] cancel",
            Style::new()
                .fg(THEME_WARNING)
                .bg(THEME_PANEL)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    Paragraph::new(lines)
        .block(
            Block::bordered()
                .style(surface_style())
                .border_style(Style::new().fg(THEME_ERROR).bg(THEME_BG))
                .title(Line::from(Span::styled(
                    match app.confirm_kind {
                        Some(ConfirmKind::Quit) => "Quit?",
                        Some(ConfirmKind::Operation) | None => "Confirm",
                    },
                    Style::new()
                        .fg(THEME_ERROR)
                        .bg(THEME_BG)
                        .add_modifier(Modifier::BOLD),
                ))),
        )
        .style(surface_style())
        .wrap(Wrap { trim: true })
}

pub(crate) fn confirm_body_lines(app: &TuiApp) -> Vec<String> {
    let mut lines = app.confirm_lines.clone();
    if lines
        .last()
        .is_some_and(|line| line.contains("press y") || line.contains("confirm"))
    {
        lines.pop();
    }
    lines
}

pub(crate) fn sort_widget(app: &TuiApp) -> Paragraph<'_> {
    let options = [
        (SortKey::Name, "n", "Name"),
        (SortKey::Modified, "m", "Modified"),
        (SortKey::Size, "z", "Size"),
        (SortKey::Type, "t", "Type"),
    ];
    let mut lines = vec![Line::raw(app.sort_mode.label()).style(confirm_style())];
    lines.push(Line::raw("").style(surface_style()));
    for (key, shortcut, label) in options {
        let active = app.sort_mode.key == key;
        let marker = if active {
            app.sort_mode.direction.marker()
        } else {
            "select"
        };
        let style = if active {
            confirm_style()
        } else {
            content_style()
        };
        lines.push(Line::raw(format!("[{shortcut}] {label:<8} {marker}")).style(style));
    }
    lines.push(Line::raw("").style(surface_style()));
    lines.push(Line::raw("Esc cancel").style(meta_value_style()));

    Paragraph::new(lines)
        .block(
            Block::bordered()
                .style(surface_style())
                .border_style(Style::new().fg(THEME_ACCENT).bg(THEME_BG))
                .title(Line::from(Span::styled(
                    "Sort",
                    Style::new()
                        .fg(THEME_ACCENT)
                        .bg(THEME_BG)
                        .add_modifier(Modifier::BOLD),
                ))),
        )
        .style(surface_style())
}

pub(crate) fn input_popup(app: &TuiApp) -> Option<InputPopup<'_>> {
    match app.mode {
        AppMode::Command => Some(InputPopup {
            title: "Command",
            prompt: ":",
            input: &app.command_input,
            anchor: InputPopupAnchor::TopCenter,
        }),
        AppMode::Filter => Some(InputPopup {
            title: "Filter",
            prompt: "",
            input: &app.filter_input,
            anchor: InputPopupAnchor::TopCenter,
        }),
        AppMode::Search => Some(InputPopup {
            title: "Search",
            prompt: "",
            input: &app.search_input,
            anchor: InputPopupAnchor::TopCenter,
        }),
        AppMode::Rename => Some(InputPopup {
            title: "Rename",
            prompt: "",
            input: &app.rename_input,
            anchor: InputPopupAnchor::SelectedFile,
        }),
        AppMode::Normal | AppMode::Sort | AppMode::Confirm => None,
    }
}

pub(crate) fn input_popup_area(
    root: Rect,
    files: Rect,
    app: &TuiApp,
    anchor: InputPopupAnchor,
) -> Rect {
    const WIDTH: u16 = 50;
    const HEIGHT: u16 = 3;
    const TOP_OFFSET: u16 = 2;
    const HOVERED_Y_OFFSET: u16 = 1;

    if anchor == InputPopupAnchor::SelectedFile {
        if let Some(area) =
            selected_file_input_area(root, files, app, WIDTH, HEIGHT, HOVERED_Y_OFFSET)
        {
            return area;
        }
    }

    top_centered_rect(root, WIDTH, HEIGHT, TOP_OFFSET)
}

pub(crate) fn selected_file_input_area(
    root: Rect,
    files: Rect,
    app: &TuiApp,
    width: u16,
    height: u16,
    y_offset: u16,
) -> Option<Rect> {
    let rows = files_visible_rows(files);
    let visible_indices = app.filtered_indices();
    let range = app.visible_entry_range(rows);
    let selected_row = app.selected_index().and_then(|selected| {
        visible_indices[range.clone()]
            .iter()
            .position(|index| *index == selected)
    })?;
    let files_inner = inner_rect(files);
    let y = files_inner
        .y
        .saturating_add(selected_row as u16)
        .saturating_add(y_offset);
    if y.saturating_add(height) > files.bottom() {
        return None;
    }

    let width = width.min(root.width);
    let height = height.min(root.height);
    let centered_x = files.x + files.width.saturating_sub(width) / 2;
    let max_x = root.right().saturating_sub(width);
    Some(Rect {
        x: centered_x.min(max_x),
        y,
        width,
        height,
    })
}

pub(crate) fn input_popup_widget(input: InputPopup<'_>) -> Paragraph<'_> {
    Paragraph::new(Line::from(vec![
        Span::styled(input.prompt, confirm_style()),
        Span::styled(input.input, content_style()),
        Span::styled("|", confirm_style()),
    ]))
    .block(
        Block::bordered()
            .style(surface_style())
            .border_style(Style::new().fg(THEME_ACCENT).bg(THEME_BG))
            .title(Line::from(Span::styled(
                input.title,
                Style::new()
                    .fg(THEME_ACCENT)
                    .bg(THEME_BG)
                    .add_modifier(Modifier::BOLD),
            ))),
    )
    .style(surface_style())
}

pub(crate) fn status_widget(app: &TuiApp) -> Paragraph<'_> {
    let mut spans = vec![
        Span::styled(
            app.mode.label(),
            Style::new()
                .fg(THEME_BG)
                .bg(THEME_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG)),
        Span::styled(
            app.position_label(),
            Style::new().fg(THEME_TEXT).bg(THEME_BG),
        ),
        Span::styled("  ", Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG)),
        Span::styled(
            format!("{} selected", app.selected_paths.len()),
            Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG),
        ),
        Span::styled("  ", Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG)),
        Span::styled(
            app.sort_mode.label(),
            Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG),
        ),
        Span::styled("  ", Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG)),
        Span::styled(
            app.path().to_string(),
            Style::new().fg(THEME_TEXT).bg(THEME_BG),
        ),
    ];
    if let Some(hint) = app.status_hint() {
        spans.push(Span::styled(
            "  ",
            Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG),
        ));
        spans.push(Span::styled(
            hint,
            Style::new()
                .fg(request_state_color(app.request_state))
                .bg(THEME_BG),
        ));
    }
    Paragraph::new(Line::from(spans)).style(Style::new().fg(THEME_STATUS_TEXT).bg(THEME_BG))
}

pub(crate) fn request_state_color(state: DirectoryRequestState) -> Color {
    match state {
        DirectoryRequestState::Idle => THEME_SUCCESS,
        DirectoryRequestState::Loading | DirectoryRequestState::Refreshing => THEME_WARNING,
        DirectoryRequestState::Failed => THEME_ERROR,
    }
}

pub(crate) fn file_size_label(entry: &FileEntry) -> String {
    if entry.is_dir {
        return "<DIR>".to_string();
    }

    let bytes = u64::try_from(entry.size).unwrap_or(0);
    bytes_label(bytes)
}

pub(crate) fn bytes_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len().saturating_sub(1) {
        value /= 1024.0;
        unit_index = unit_index.saturating_add(1);
    }
    format!("{value:.1} {}", UNITS[unit_index])
}

pub(crate) fn modified_time_label(timestamp: i64) -> String {
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp(timestamp) else {
        return "unknown".to_string();
    };
    datetime
        .format(format_description!("[year]-[month]-[day] [hour]:[minute]"))
        .unwrap_or_else(|_| "unknown".to_string())
}

pub(crate) fn files_empty_message(app: &TuiApp) -> String {
    if !app.filter_input.is_empty() {
        return format!("No matches for \"{}\"", app.filter_input);
    }
    if let Some(state) = app.search_results_state() {
        return if state.running {
            format!("Searching \"{}\"...", state.keyword)
        } else {
            format!("No matches for \"{}\"", state.keyword)
        };
    }
    match app.request_state {
        DirectoryRequestState::Loading => "Loading...".to_string(),
        DirectoryRequestState::Refreshing => "Refreshing cached entries...".to_string(),
        DirectoryRequestState::Failed => app.notice.as_deref().map_or_else(
            || "Could not load directory".to_string(),
            |notice| format!("Error: {notice}"),
        ),
        DirectoryRequestState::Idle => "Empty directory".to_string(),
    }
}

pub(crate) fn task_status_line(app: &TuiApp) -> Option<Line<'static>> {
    match app.request_state {
        DirectoryRequestState::Loading => Some(
            Line::raw(format!("Loading {}", app.path())).style(
                Style::new()
                    .fg(request_state_color(app.request_state))
                    .bg(THEME_PANEL),
            ),
        ),
        DirectoryRequestState::Refreshing => Some(
            Line::raw(format!("Refreshing {} from cache", app.path())).style(
                Style::new()
                    .fg(request_state_color(app.request_state))
                    .bg(THEME_PANEL),
            ),
        ),
        DirectoryRequestState::Failed => app.notice.as_deref().map(|notice| {
            Line::raw(format!("Error: {notice}")).style(
                Style::new()
                    .fg(request_state_color(app.request_state))
                    .bg(THEME_PANEL),
            )
        }),
        DirectoryRequestState::Idle => (app.background_tasks > 0)
            .then(|| {
                Line::raw(format!(
                    "{} {} running",
                    app.background_tasks,
                    pluralize(app.background_tasks, "task", "tasks")
                ))
                .style(meta_value_style())
            })
            .or_else(|| {
                app.search_results_state().map(|state| {
                    Line::raw(format!(
                        "{} matches for \"{}\" in {} ({} dirs, {} entries)",
                        state.matches,
                        state.keyword,
                        state.root_path,
                        state.scanned_dirs,
                        state.scanned_entries
                    ))
                    .style(meta_value_style())
                })
            }),
    }
}

pub(crate) const fn pluralize(
    count: usize,
    singular: &'static str,
    plural: &'static str,
) -> &'static str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

pub(crate) fn truncate_cells(value: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let budget = max_width - 3;
    let mut output = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + width > budget {
            break;
        }
        output.push(ch);
        used += width;
    }
    output.push_str("...");
    output
}

pub(crate) fn marquee_cells(value: &str, max_width: usize, tick: u64) -> String {
    let width = UnicodeWidthStr::width(value);
    if width <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }

    let offset = marquee_offset(width, max_width, tick);
    cell_window(value, offset, max_width)
}

pub(crate) fn marquee_offset(width: usize, max_width: usize, tick: u64) -> usize {
    let overflow = width.saturating_sub(max_width);
    if overflow == 0 {
        return 0;
    }

    let speed = 1 + (overflow / 32).min(4);
    let move_ticks = overflow.div_ceil(speed).max(1) as u64;
    let pause = MARQUEE_EDGE_PAUSE_TICKS;
    let cycle = pause
        .saturating_add(move_ticks)
        .saturating_add(pause)
        .saturating_add(move_ticks);
    let mut phase = tick % cycle.max(1);
    if phase < pause {
        return 0;
    }
    phase = phase.saturating_sub(pause);
    if phase < move_ticks {
        return (phase as usize).saturating_mul(speed).min(overflow);
    }
    phase = phase.saturating_sub(move_ticks);
    if phase < pause {
        return overflow;
    }
    phase = phase.saturating_sub(pause);
    overflow.saturating_sub((phase as usize).saturating_mul(speed).min(overflow))
}

pub(crate) fn cell_window(value: &str, start: usize, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut output = String::new();
    let mut cursor: usize = 0;
    let mut used: usize = 0;
    for ch in value.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        let next_cursor = cursor.saturating_add(width);
        if width > 0 && next_cursor <= start {
            cursor = next_cursor;
            continue;
        }
        if used.saturating_add(width) > max_width {
            break;
        }
        output.push(ch);
        used = used.saturating_add(width);
        cursor = next_cursor;
    }

    output
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviewBudget {
    pub kind: PreviewKind,
    pub fetched_bytes: u64,
    pub memory_limit_bytes: u64,
}
