use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table},
};

use crate::app::{App, FocusPanel};
use crate::slurm::JobState;

/// Border style for focused vs unfocused panels
fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

pub fn draw_ui(f: &mut Frame, app: &mut App) {
    // Lazily fetch scontrol details for the selected job
    app.ensure_job_details();

    let main_and_status = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(f.area());

    let main_area = main_and_status[0];
    let status_area = main_and_status[1];

    // Main content: jobs left, details+stdout right
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(main_area);

    // Store rects for mouse hit testing
    app.job_list_area = h_chunks[0];

    draw_job_list(f, app, h_chunks[0]);

    // Right side: details on top, stdout preview below
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(5)])
        .split(h_chunks[1]);

    app.log_area = v_chunks[1];

    draw_details(f, app, v_chunks[0]);
    draw_stdout_preview(f, app, v_chunks[1]);
    draw_status_bar(f, app, status_area);
}

fn draw_job_list(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let focused = app.focus == FocusPanel::Jobs;
    let header_cells = ["", "JobID", "Part", "User", "Time", "Name"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = app
        .jobs
        .iter()
        .map(|job| {
            let state_color = match job.state {
                JobState::Running => Color::Green,
                JobState::Pending => Color::Yellow,
                JobState::Failed => Color::Red,
                JobState::Cancelled => Color::Gray,
                _ => Color::White,
            };

            let cells = vec![
                Cell::from(job.state.as_str()).style(Style::default().fg(state_color)),
                Cell::from(job.job_id.as_str()),
                Cell::from(job.partition.as_str()),
                Cell::from(job.user.as_str()),
                Cell::from(job.time.as_str()),
                Cell::from(job.name.as_str()),
            ];
            Row::new(cells)
        })
        .collect();

    let job_count = app.jobs.len();
    let title = format!(" Jobs ({}) ", job_count);

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style(focused)),
    )
    .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_details(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let detail_text = if let Some(job) = app.selected_job() {
        let state_color = match job.state {
            JobState::Running => Color::Green,
            JobState::Pending => Color::Yellow,
            JobState::Failed => Color::Red,
            _ => Color::White,
        };

        let state_str = format!("{:?}", job.state);
        let stderr_str = job.stderr.clone().unwrap_or_default();
        let stdout_str = job.stdout.clone().unwrap_or_default();

        // Highlight the currently active log source in cyan
        let stderr_color = if app.show_stderr { Some(Color::Cyan) } else { None };
        let stdout_color = if !app.show_stderr { Some(Color::Cyan) } else { None };

        vec![
            detail_line("State    ", &state_str, Some(state_color)),
            detail_line("Name     ", &job.name, None),
            detail_line("Command  ", &job.command, None),
            detail_line("Nodes    ", &job.nodelist, None),
            detail_line("TRES     ", &job.tres, None),
            detail_line("WorkDir  ", &job.work_dir, None),
            detail_line("stderr   ", &stderr_str, stderr_color),
            detail_line("stdout   ", &stdout_str, stdout_color),
        ]
    } else {
        vec![Line::from("No job selected")]
    };

    let details = Paragraph::new(detail_text)
        .block(Block::default().borders(Borders::ALL).title(" Details "));

    f.render_widget(details, area);
}

fn draw_stdout_preview(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let focused = app.focus == FocusPanel::Log;
    let label = if app.show_stderr { "stderr" } else { "stdout" };
    let path_str = if let Some(job) = app.selected_job() {
        if app.show_stderr {
            job.stderr.as_deref().unwrap_or("stderr")
        } else {
            job.stdout.as_deref().unwrap_or("stdout")
        }
    } else {
        label
    };

    // Line range indicator: [L1-30/500] showing visible range
    let viewport_lines = area.height.saturating_sub(2) as usize;
    let scroll_info = if app.log_line_count > 0 {
        let first_visible = app.log_scroll as usize + 1;
        let last_visible = (app.log_scroll as usize + viewport_lines).min(app.log_line_count);
        format!(" [L{}-{}/{}]", first_visible, last_visible, app.log_line_count)
    } else {
        String::new()
    };
    let title = format!(" {}: {}{} ", label, path_str, scroll_info);

    let (content, style) = if let Some(ref error) = app.log_error {
        (
            format!("Read error: {}", error),
            Style::default().fg(Color::Red),
        )
    } else if let Some(ref log) = app.log_preview {
        (log.clone(), Style::default().fg(Color::White))
    } else {
        (
            "Loading...".to_string(),
            Style::default().fg(Color::DarkGray),
        )
    };

    let log_widget = Paragraph::new(content)
        .style(style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style(focused)),
        )
        .scroll((app.log_scroll, 0));

    f.render_widget(log_widget, area);

    // Scrollbar on the right edge of the log panel
    if app.log_line_count > viewport_lines {
        let max_scroll = app.log_line_count.saturating_sub(viewport_lines);
        let mut scrollbar_state = ScrollbarState::new(max_scroll)
            .position(app.log_scroll as usize);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█");

        // Render scrollbar inside the block's border area
        f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let key = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let sep = Span::raw("  ");

    let lines = match app.focus {
        FocusPanel::Jobs => {
            let toggle_label = if app.show_stderr { "stdout" } else { "stderr" };
            vec![
                Line::from(vec![
                    Span::styled(" q", key), Span::raw(" quit"), sep.clone(),
                    Span::styled("j/k", key), Span::raw(" navigate"), sep.clone(),
                    Span::styled("g/G", key), Span::raw(" top/bottom"), sep.clone(),
                    Span::styled(&app.config.keybindings.toggle_logs, key),
                    Span::raw(format!(" toggle {}", toggle_label)), sep.clone(),
                    Span::styled(&app.config.keybindings.refresh, key), Span::raw(" refresh"),
                ]),
                Line::from(vec![
                    Span::styled(" Tab", key), Span::raw("/"),
                    Span::styled("Enter", key), Span::raw(" focus log"), sep.clone(),
                    Span::styled("^d/^u", key), Span::raw(" scroll log"), sep.clone(),
                    Span::raw("mouse: click panel or scroll wheel"),
                ]),
            ]
        }
        FocusPanel::Log => {
            let toggle_label = if app.show_stderr { "stdout" } else { "stderr" };
            vec![
                Line::from(vec![
                    Span::styled(" LOG FOCUS", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    sep.clone(),
                    Span::styled("j/k", key), Span::raw("/"),
                    Span::styled("↑↓", key), Span::raw(" scroll"), sep.clone(),
                    Span::styled("g/G", key), Span::raw(" top/bottom"), sep.clone(),
                    Span::styled("PgUp/PgDn", key), Span::raw(" page"), sep.clone(),
                    Span::styled("^d/^u", key), Span::raw(" half-page"),
                ]),
                Line::from(vec![
                    Span::styled(" Esc", key), Span::raw("/"),
                    Span::styled("Tab", key), Span::raw(" back to jobs"), sep.clone(),
                    Span::styled(&app.config.keybindings.toggle_logs, key),
                    Span::raw(format!(" toggle {}", toggle_label)), sep.clone(),
                    Span::styled("q", key), Span::raw(" quit"),
                ]),
            ]
        }
    };

    let status = Paragraph::new(lines)
        .style(Style::default().bg(Color::DarkGray));

    f.render_widget(status, area);
}

fn detail_line(label: &str, value: &str, value_color: Option<Color>) -> Line<'static> {
    let val_style = match value_color {
        Some(c) => Style::default().fg(c),
        None => Style::default(),
    };
    Line::from(vec![
        Span::styled(label.to_string(), Style::default().fg(Color::Yellow)),
        Span::styled(value.to_string(), val_style),
    ])
}
