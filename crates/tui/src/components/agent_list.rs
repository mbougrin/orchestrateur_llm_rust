use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use tokenmind_core::task::TaskStatus;
use crate::app::App;

const SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title("AGENTS ACTIFS")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let running_tasks: Vec<_> = app.task_history.iter()
        .filter(|t| t.status == TaskStatus::Running)
        .collect();

    let mut lines: Vec<Line> = Vec::new();

    if running_tasks.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No active agents",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_millis() / 125;
        let spinner = SPINNER_FRAMES[(tick as usize) % SPINNER_FRAMES.len()];

        for task in running_tasks {
            let model_name = task.assigned_model.display_name();
            let desc = if task.description.len() > 28 {
                format!("{}…", &task.description[..28])
            } else {
                task.description.clone()
            };

            let color = match task.assigned_model {
                llm_clients::LlmModel::ClaudeSonnet => Color::Magenta,
                llm_clients::LlmModel::ClaudeHaiku  => Color::Cyan,
                llm_clients::LlmModel::Gemini        => Color::Green,
                llm_clients::LlmModel::Grok          => Color::LightRed,
                llm_clients::LlmModel::Gpt           => Color::Blue,
                llm_clients::LlmModel::Local         => Color::Yellow,
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", spinner), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(model_name, Style::default().fg(color).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(format!("\"{}\"", desc), Style::default().fg(Color::White)),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}
