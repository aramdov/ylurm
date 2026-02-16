use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::app::App;
use crate::slurm::JobState;

pub fn draw_ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(f.area());

    draw_job_list(f, app, chunks[0]);
    draw_details(f, app, chunks[1]);
}

fn draw_job_list(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let header_cells = ["", "JobID", "Part", "User", "Time", "Name"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = app
        .jobs
        .iter()
        .enumerate()
        .map(|(_i, job)| {
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
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_details(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let detail_text = if let Some(idx) = app.table_state.selected() {
        if let Some(job) = app.jobs.get(idx) {
            let state_color = match job.state {
                JobState::Running => Color::Green,
                JobState::Pending => Color::Yellow,
                JobState::Failed => Color::Red,
                _ => Color::White,
            };

            vec![
                Line::from(vec![
                    Span::styled("State    ", Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{:?}", job.state), Style::default().fg(state_color)),
                ]),
                Line::from(vec![
                    Span::styled("Name     ", Style::default().fg(Color::Yellow)),
                    Span::raw(&job.name),
                ]),
                Line::from(vec![
                    Span::styled("Command  ", Style::default().fg(Color::Yellow)),
                    Span::raw(&job.command),
                ]),
                Line::from(vec![
                    Span::styled("Nodes    ", Style::default().fg(Color::Yellow)),
                    Span::raw(&job.nodelist),
                ]),
                Line::from(vec![
                    Span::styled("TRES     ", Style::default().fg(Color::Yellow)),
                    Span::raw(&job.tres),
                ]),
                Line::from(vec![
                    Span::styled("stderr   ", Style::default().fg(Color::Yellow)),
                    Span::raw(&job.stderr),
                ]),
                Line::from(vec![
                    Span::styled("WorkDir  ", Style::default().fg(Color::Yellow)),
                    Span::raw(&job.work_dir),
                ]),
            ]
        } else {
            vec![Line::from("No job selected")]
        }
    } else {
        vec![Line::from("No job selected")]
    };

    let details = Paragraph::new(detail_text)
        .block(Block::default().borders(Borders::ALL).title(" Details "));

    f.render_widget(details, area);
}
