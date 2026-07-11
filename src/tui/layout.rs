use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TuiLayout {
    pub(crate) files: Rect,
    pub(crate) preview: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FullLayout {
    pub(crate) header: Rect,
    pub(crate) body: Rect,
    pub(crate) tasks: Rect,
    pub(crate) transfer: Rect,
    pub(crate) status: Rect,
}

pub(crate) fn full_layout(area: Rect, has_transfer_area: bool) -> FullLayout {
    if has_transfer_area {
        let [header, body, tasks, transfer, status] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(4),
                Constraint::Length(4),
                Constraint::Length(4),
                Constraint::Length(1),
            ])
            .areas(area);
        return FullLayout {
            header,
            body,
            tasks,
            transfer,
            status,
        };
    }

    let [header, body, tasks, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(area);
    FullLayout {
        header,
        body,
        tasks,
        transfer: Rect::default(),
        status,
    }
}

pub(crate) fn browser_layout(body: Rect) -> TuiLayout {
    if body.width >= 88 {
        let [files, preview] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Min(42)])
            .areas(body);
        TuiLayout { files, preview }
    } else if body.width >= 56 {
        let [files, preview] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(54), Constraint::Percentage(46)])
            .areas(body);
        TuiLayout { files, preview }
    } else {
        let [files, preview] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .areas(body);
        TuiLayout { files, preview }
    }
}

pub(crate) fn inner_rect(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

pub(crate) fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

pub(crate) fn vertical_scrollbar_rect(inner: Rect) -> Option<Rect> {
    (inner.width > 0 && inner.height > 0)
        .then(|| Rect::new(inner.right().saturating_sub(1), inner.y, 1, inner.height))
}

pub(crate) fn scrollbar_position_for_offset(total: usize, rows: usize, offset: usize) -> usize {
    if total == 0 || rows == 0 {
        return 0;
    }

    let rows = rows.min(total);
    let max_start = total.saturating_sub(rows);
    if max_start == 0 {
        return 0;
    }

    proportional_position(offset.min(max_start), total.saturating_sub(1), max_start)
}

pub(crate) fn scrollbar_offset_at_row(
    total: usize,
    rows: usize,
    area: Rect,
    row: u16,
) -> Option<usize> {
    if total == 0 || rows == 0 || total <= rows {
        return None;
    }

    let rows = rows.min(total);
    let max_start = total.saturating_sub(rows);
    if row <= area.y {
        return Some(0);
    }

    let last_row = area.bottom().saturating_sub(1);
    if row >= last_row {
        return Some(max_start);
    }

    let track_len = usize::from(area.height.saturating_sub(2));
    if track_len <= 1 {
        return Some(max_start / 2);
    }

    let track_row =
        usize::from(row.saturating_sub(area.y.saturating_add(1))).min(track_len.saturating_sub(1));
    Some(proportional_position(
        track_row,
        max_start,
        track_len.saturating_sub(1),
    ))
}

pub(crate) fn proportional_position(value: usize, max_value: usize, max_input: usize) -> usize {
    if value == 0 || max_value == 0 || max_input == 0 {
        return 0;
    }

    (((value as u128) * (max_value as u128) + ((max_input as u128) / 2)) / (max_input as u128))
        as usize
}

pub fn render_app(frame: &mut Frame<'_>, app: &TuiApp, preview: Option<PreviewStatus>) {
    frame.render_widget(Block::new().style(surface_style()), frame.area());

    let layout = full_layout(frame.area(), !app.task_items.is_empty());
    let browser = browser_layout(layout.body);
    frame.render_widget(header_widget(app), layout.header);
    render_files_pane(frame, app, browser.files);
    render_preview_pane(frame, app, preview, browser.preview);
    render_tasks_pane(frame, app, layout.tasks);
    render_transfer_pane(frame, app, layout.transfer);
    frame.render_widget(status_widget(app), layout.status);
    if let Some(input) = input_popup(app) {
        let area = input_popup_area(frame.area(), browser.files, app, input.anchor);
        frame.render_widget(Clear, area);
        frame.render_widget(input_popup_widget(input), area);
    }
    if app.help_visible {
        let area = centered_rect(frame.area(), 58, 22);
        frame.render_widget(Clear, area);
        frame.render_widget(help_widget(), area);
    }
    if !app.confirm_lines.is_empty() {
        let body_lines = confirm_body_lines(app).len() as u16;
        let area = centered_rect(frame.area(), 64, body_lines.saturating_add(4).clamp(7, 14));
        frame.render_widget(Clear, area);
        frame.render_widget(confirm_widget(app), area);
    }
    if app.mode == AppMode::Sort {
        let area = centered_rect(frame.area(), 48, 9);
        frame.render_widget(Clear, area);
        frame.render_widget(sort_widget(app), area);
    }
}
