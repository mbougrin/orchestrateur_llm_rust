use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let has_anthropic = !app.ctx.anthropic_key.is_empty();
    let has_gemini = !app.ctx.gemini_key.is_empty();
    let model_loaded = local_llm::is_model_loaded();

    let local_model = std::env::var("MODEL_FILE")
        .unwrap_or_else(|_| "qwen2.5-coder (auto RAM)".to_string());

    let lines = vec![
        Line::from(vec![
            Span::raw("  Anthropic  : "),
            Span::styled(
                if has_anthropic { "✓ configuré" } else { "✗ ANTHROPIC_API_KEY manquante" },
                Style::default().fg(if has_anthropic { Color::Green } else { Color::Red }),
            ),
        ]),
        Line::from(vec![
            Span::raw("  Gemini     : "),
            Span::styled(
                if has_gemini { "✓ configuré" } else { "✗ GEMINI_API_KEY manquante" },
                Style::default().fg(if has_gemini { Color::Green } else { Color::Red }),
            ),
        ]),
        Line::from(vec![
            Span::raw("  llama-cpp-4: "),
            Span::styled(
                if model_loaded { "✓ modèle chargé" } else { "○ chargement à la demande" },
                Style::default().fg(if model_loaded { Color::Green } else { Color::Yellow }),
            ),
        ]),
        Line::from(vec![
            Span::raw("  Modèle     : "),
            Span::styled(local_model, Style::default().fg(Color::Cyan)),
        ]),
    ];

    let block = Block::default()
        .title("MODEL STATUS")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    f.render_widget(Paragraph::new(lines).block(block), area);
}
