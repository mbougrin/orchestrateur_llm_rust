use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title("LOGS  [Tab: toggle]")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner_height = area.height.saturating_sub(2) as usize;
    let start = app.log_messages.len().saturating_sub(inner_height);
    let visible = &app.log_messages[start..];

    let lines: Vec<Line> = visible.iter()
        .map(|msg| {
            let color = if msg.starts_with("✗") || msg.contains("FAILED") || msg.contains("Error") {
                Color::Red
            } else if msg.starts_with("✓") || msg.contains("Done") || msg.contains("OK") {
                Color::Green
            } else if msg.starts_with(">") {
                Color::Cyan
            } else {
                Color::White
            };
            Line::styled(msg.clone(), Style::default().fg(color))
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}
