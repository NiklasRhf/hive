use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use ratatui::Terminal;
use std::io;

#[derive(serde::Deserialize, serde::Serialize)]
struct Notification {
    pane: String,
    message: String,
}

fn load_notifications() -> Result<Vec<Notification>> {
    let content = std::fs::read_to_string(crate::NOTIF_FILE)
        .context("no notifications file")?;
    Ok(serde_json::from_str(&content)?)
}

fn save_notifications(notifs: &[Notification]) -> Result<()> {
    std::fs::write(crate::NOTIF_FILE, serde_json::to_string(notifs)?)?;
    Ok(())
}

pub fn run() -> Result<()> {
    let mut notifs = load_notifications()?;
    if notifs.is_empty() {
        return Ok(());
    }

    terminal::enable_raw_mode()?;
    while event::poll(std::time::Duration::from_millis(50))? {
        let _ = event::read()?;
    }

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, &mut notifs);
    terminal::disable_raw_mode()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    notifs: &mut Vec<Notification>,
) -> Result<()> {
    let mut selected: usize = 0;

    loop {
        if notifs.is_empty() {
            return Ok(());
        }

        let count = notifs.len();

        terminal.draw(|f| {
            let chunks = Layout::vertical([
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(f.area());

            let items: Vec<ListItem> = notifs
                .iter()
                .enumerate()
                .map(|(i, notif)| {
                    let style = if i == selected {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(&notif.message, style),
                    ]))
                })
                .collect();

            let visible_count = items.len().min(chunks[0].height as usize);
            let skip = items.len().saturating_sub(visible_count);
            let visible_items: Vec<ListItem> = items.into_iter().skip(skip).collect();
            f.render_widget(
                List::new(visible_items),
                ratatui::layout::Rect {
                    y: chunks[0].y + chunks[0].height.saturating_sub(visible_count as u16),
                    height: visible_count as u16,
                    ..chunks[0]
                },
            );

            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {} notification(s)", count),
                    Style::default().fg(Color::Cyan),
                ))),
                chunks[1],
            );

            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "enter: jump | d: delete | esc: close",
                    Style::default().fg(Color::Green),
                ))),
                chunks[2],
            );
        })?;

        if let Event::Key(key) = event::read()? {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(()),
                (_, KeyCode::Char('d')) => {
                    if selected < notifs.len() {
                        notifs.remove(selected);
                        save_notifications(notifs)?;
                        if selected > 0 && selected >= notifs.len() {
                            selected = notifs.len().saturating_sub(1);
                        }
                    }
                }
                (_, KeyCode::Enter) => {
                    if let Some(notif) = notifs.get(selected) {
                        if let Some((session, window)) = notif.pane.rsplit_once(':') {
                            let _ = crate::tmux::switch_client(session);
                            let _ = std::process::Command::new("tmux")
                                .args(["select-window", "-t", &format!("{session}:{window}")])
                                .status();
                        }
                        return Ok(());
                    }
                }
                (_, KeyCode::Up) => selected = selected.saturating_sub(1),
                (_, KeyCode::Down) => {
                    if selected + 1 < notifs.len() {
                        selected += 1;
                    }
                }
                _ => {}
            }
        }
    }
}
