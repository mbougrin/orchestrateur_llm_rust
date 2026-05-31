use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use similar::{ChangeTag, TextDiff};
use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title("DIFF — dernière modification (Tab: retour)")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    if app.diff_history.is_empty() {
        let p = Paragraph::new("Aucun diff disponible. Les diffs apparaissent après chaque écriture.")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        f.render_widget(p, area);
        return;
    }

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Show all diffs from diff_history (most recent last)
    let mut lines: Vec<Line> = Vec::new();
    let max_lines = inner.height as usize;

    for (path, old, new) in &app.diff_history {
        lines.push(Line::from(Span::styled(
            format!("─── {} ───", path),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));

        let diff = TextDiff::from_lines(old.as_str(), new.as_str());
        for change in diff.iter_all_changes() {
            let (prefix, style) = match change.tag() {
                ChangeTag::Delete => ("-", Style::default().fg(Color::Red)),
                ChangeTag::Insert => ("+", Style::default().fg(Color::Green)),
                ChangeTag::Equal  => (" ", Style::default().fg(Color::DarkGray)),
            };
            let content = change.value().trim_end_matches('\n');
            if change.tag() != ChangeTag::Equal || lines.len() < 5 {
                lines.push(Line::from(Span::styled(
                    format!("{}{}", prefix, content),
                    style,
                )));
            }
            if lines.len() > max_lines * 3 { break; }
        }
        lines.push(Line::from(""));
    }

    // Show tail of lines that fit
    let visible_start = lines.len().saturating_sub(max_lines);
    let visible: Vec<Line> = lines[visible_start..].to_vec();

    f.render_widget(
        Paragraph::new(visible).wrap(Wrap { trim: false }),
        inner,
    );
}
