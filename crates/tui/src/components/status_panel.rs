use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use crate::app::{App, ModelState};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title("STATUS")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 { return; }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Anthropic
            Constraint::Length(1), // Gemini
            Constraint::Length(1), // Grok
            Constraint::Length(1), // GPT
            Constraint::Length(1), // Local model
            Constraint::Length(1), // blank separator
            Constraint::Length(1), // CPU
            Constraint::Length(1), // RAM
            Constraint::Length(1), // Swap
        ])
        .split(inner);

    // ── API keys ──────────────────────────────────────────────────────────────

    render_api(f, rows[0], "Anthropic", app.api_anthropic_ok);
    render_api(f, rows[1], "Gemini   ", app.api_gemini_ok);
    render_api(f, rows[2], "Grok     ", app.api_grok_ok);
    render_api(f, rows[3], "GPT      ", app.api_gpt_ok);

    if rows[4].y < inner.y + inner.height {
        let (label, color) = match &app.model_state {
            ModelState::Idle        => ("idle",          Color::DarkGray),
            ModelState::Downloading => ("downloading…",  Color::Yellow),
            ModelState::Loading     => ("loading…",      Color::Cyan),
            ModelState::Ready       => ("ready",         Color::Green),
            ModelState::Failed(_)   => ("failed",        Color::Red),
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Local    ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("[{}]", label),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ])),
            rows[4],
        );
    }

    // rows[5] blank separator — skip

    // ── System metrics ────────────────────────────────────────────────────────

    let cpu_pct  = app.sys.global_cpu_usage() as f64;
    let ram_used = app.sys.used_memory();
    let ram_tot  = app.sys.total_memory();
    let swp_used = app.sys.used_swap();
    let swp_tot  = app.sys.total_swap();

    if rows[6].y < inner.y + inner.height {
        let ratio = (cpu_pct / 100.0).clamp(0.0, 1.0);
        render_metric(f, rows[6], "CPU ",
            &format!("{:.0}%", cpu_pct),
            ratio, cpu_color(ratio));
    }

    if rows[7].y < inner.y + inner.height {
        let ratio = if ram_tot > 0 { (ram_used as f64 / ram_tot as f64).clamp(0.0, 1.0) } else { 0.0 };
        render_metric(f, rows[7], "RAM ",
            &format!("{:.1}/{:.1}G", to_gb(ram_used), to_gb(ram_tot)),
            ratio, mem_color(ratio));
    }

    if rows[8].y < inner.y + inner.height {
        let ratio = if swp_tot > 0 { (swp_used as f64 / swp_tot as f64).clamp(0.0, 1.0) } else { 0.0 };
        render_metric(f, rows[8], "Swap",
            &format!("{:.1}/{:.1}G", to_gb(swp_used), to_gb(swp_tot)),
            ratio, mem_color(ratio));
    }
}

fn render_api(f: &mut Frame, area: Rect, label: &str, ok: bool) {
    let (badge, color) = if ok {
        ("[✓ ok]   ", Color::Green)
    } else {
        ("[✗ absent]", Color::Red)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} ", label), Style::default().fg(Color::White)),
            Span::styled(badge, Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ])),
        area,
    );
}

/// Renders a compact metric line: "  LABEL [████░░░░] value"
fn render_metric(f: &mut Frame, area: Rect, label: &str, value: &str, ratio: f64, color: Color) {
    let bar_width: usize = 8;
    let filled = ((ratio * bar_width as f64).round() as usize).min(bar_width);
    let empty  = bar_width - filled;
    let bar    = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} ", label), Style::default().fg(Color::White)),
            Span::styled(format!("[{}]", bar), Style::default().fg(color)),
            Span::styled(format!(" {}", value), Style::default().fg(Color::White)),
        ])),
        area,
    );
}

fn to_gb(bytes: u64) -> f64 { bytes as f64 / 1_073_741_824.0 }

fn cpu_color(r: f64) -> Color {
    if r < 0.5 { Color::Green } else if r < 0.8 { Color::Yellow } else { Color::Red }
}

fn mem_color(r: f64) -> Color {
    if r < 0.6 { Color::Cyan } else if r < 0.85 { Color::Yellow } else { Color::Red }
}
