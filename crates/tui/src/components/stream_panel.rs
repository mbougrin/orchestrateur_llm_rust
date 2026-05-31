use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title("STREAM EN DIRECT")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Pick the most recent active stream buffer
    let content = app.active_streams.values().last().map(|s| s.as_str()).unwrap_or("");

    // Show last N chars that fit in the visible area, plus blinking cursor ▌
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width  = area.width.saturating_sub(2) as usize;
    let max_chars    = inner_height * inner_width;

    let visible = if content.len() > max_chars {
        &content[content.len() - max_chars..]
    } else {
        content
    };

    // Blinking ▌ based on 500 ms period
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_millis();
    let cursor = if millis < 500 { "▌" } else { " " };

    let display = format!("{}{}", visible, cursor);

    let paragraph = Paragraph::new(display)
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::White))
        .scroll((0, 0));

    // Highlight the cursor at the end
    let lines: Vec<Line> = {
        let text_part = visible;
        let last_line_len = text_part.lines().last().map(|l| l.len()).unwrap_or(0);
        let _ = last_line_len; // used for future cursor positioning

        vec![Line::from(vec![
            Span::raw(visible),
            Span::styled(cursor, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ])]
    };

    let p = Paragraph::new(lines)
        .block(Block::default()
            .title("STREAM EN DIRECT")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)))
        .wrap(Wrap { trim: false });

    f.render_widget(p, area);
    let _ = paragraph; // suppress unused warning
}
