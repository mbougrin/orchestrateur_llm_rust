use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use crate::app::{App, ModelState};
use crate::components::{agent_list, token_counter, log_panel, status_panel};

pub fn draw(f: &mut Frame, app: &App) {
    let size = f.size();

    // Outer layout: header + body + input
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header
            Constraint::Min(0),     // body
            Constraint::Length(3),  // input bar
        ])
        .split(size);

    draw_header(f, outer[0], app);
    draw_body(f, outer[1], app);
    draw_input(f, outer[2], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let project = app.ctx.project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let cost = app.ctx.total_cost();

    let (model_label, model_color) = match &app.model_state {
        ModelState::Idle        => ("local: idle",          Color::DarkGray),
        ModelState::Downloading => ("local: downloading…",  Color::Yellow),
        ModelState::Loading     => ("local: loading…",      Color::Cyan),
        ModelState::Ready       => ("local: ready",         Color::Green),
        ModelState::Failed(_)   => ("local: failed",        Color::Red),
    };

    let parts: Vec<Span> = vec![
        Span::styled(
            format!(" orchestrateur-llm v1.0  [project: {}]  [cost: ${:.4}]  [", project, cost),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(model_label, Style::default().fg(model_color).add_modifier(Modifier::BOLD)),
        Span::styled(
            "]  [Tab: logs]  [ESC: quit]",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
    ];

    f.render_widget(Paragraph::new(Line::from(parts)), area);
}

fn draw_body(f: &mut Frame, area: Rect, app: &App) {
    if app.show_logs {
        // Full-width log panel
        log_panel::render(f, area, app);
        return;
    }

    // Split: left (agents + tokens) | right (task queue)
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    // Left: status panel (API keys + system) on top, token counter below
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(body[0]);

    status_panel::render(f, left[0], app);
    token_counter::render(f, left[1], app);
    draw_task_queue(f, body[1], app);
}

fn draw_task_queue(f: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();

    // task_history is the single source of truth; statuses are updated in place.
    for task in app.task_history.iter() {
        let (symbol, color) = match task.status {
            tokenmind_core::task::TaskStatus::Done => ("✓", Color::Green),
            tokenmind_core::task::TaskStatus::Running => ("►", Color::Yellow),
            tokenmind_core::task::TaskStatus::Pending => ("○", Color::White),
            tokenmind_core::task::TaskStatus::Failed => ("✗", Color::Red),
            tokenmind_core::task::TaskStatus::Cancelled => ("–", Color::DarkGray),
        };

        let model_label = task.assigned_model.display_name();
        let desc = if task.description.len() > 40 {
            format!("{}…", &task.description[..40])
        } else {
            task.description.clone()
        };

        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", symbol), Style::default().fg(color)),
            Span::raw(format!("{} ", desc)),
            Span::styled(format!("({})", model_label), Style::default().fg(Color::DarkGray)),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No tasks. Type a prompt to start.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let block = Block::default()
        .title("TASK QUEUE")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}

fn draw_input(f: &mut Frame, area: Rect, app: &App) {
    let hint = "/btw /clear /status /cost /plan /cancel /retry /log /help";

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner area: prompt on top, hints below
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("> {}", app.input))
            .style(Style::default().fg(Color::White)),
        rows[0],
    );

    f.render_widget(
        Paragraph::new(hint)
            .style(Style::default().fg(Color::DarkGray)),
        rows[1],
    );

    // inner.x already accounts for the left border; "> " = 2 chars prefix
    f.set_cursor(
        inner.x + 2 + app.cursor_pos as u16,
        inner.y,
    );
}
