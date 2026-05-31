use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};
use ratatui::layout::{Constraint, Direction, Layout};
use llm_clients::LlmModel;
use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title("TOKEN USAGE")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let models = [
        (LlmModel::ClaudeSonnet, "Claude Sonnet", Color::Magenta),
        (LlmModel::ClaudeHaiku,  "Claude Haiku ",  Color::Cyan),
        (LlmModel::Gemini,       "Gemini Flash  ", Color::Green),
        (LlmModel::Local,        "Local Qwen    ", Color::Yellow),
    ];

    let sonnet_max = 50_000u32;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(2); models.len() + 2])
        .split(inner);

    for (i, (model, label, color)) in models.iter().enumerate() {
        if i >= rows.len() { break; }

        let tokens = app.ctx.token_count(model);

        if *model == LlmModel::Local {
            let line = Line::from(vec![
                Span::styled(format!("  {}  ", label), Style::default().fg(*color)),
                Span::styled("──────  FREE", Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(line), rows[i]);
        } else {
            let ratio = (tokens as f64 / sonnet_max as f64).min(1.0);
            let gauge = Gauge::default()
                .block(Block::default())
                .gauge_style(Style::default().fg(*color).add_modifier(Modifier::BOLD))
                .ratio(ratio)
                .label(format!("{} {:>6}", label, tokens));
            f.render_widget(gauge, rows[i]);
        }
    }

    // Cost summary
    let summary_idx = models.len();
    if summary_idx < rows.len() {
        let cost = app.ctx.total_cost();
        let savings = app.ctx.savings_percent();
        let cost_line = Line::from(vec![
            Span::styled(
                format!("  Total: ${:.4}  Saved: {:.0}%", cost, savings),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]);
        f.render_widget(Paragraph::new(cost_line), rows[summary_idx]);
    }
}
