use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use std::io;

#[derive(serde::Deserialize, serde::Serialize)]
struct Notification {
    pane: String,
    message: String,
}

fn load_notifications() -> Result<Vec<Notification>> {
    let content = std::fs::read_to_string(crate::NOTIF_FILE).context("no notifications file")?;
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
            const LOGO: [&str; 3] = [
                "█ █  █  █   █  ████",
                "███  █   █ █   █▄▄ ",
                "█ █  █    █    ████",
            ];

            let chunks = Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(f.area());

            let logo_lines: Vec<Line> = LOGO
                .iter()
                .map(|l| Line::from(Span::styled(*l, Style::default().fg(Color::Cyan))))
                .collect();
            f.render_widget(Paragraph::new(logo_lines), chunks[0]);

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

            let list_area = chunks[1];
            let visible_count = items.len().min(list_area.height as usize);
            let skip = items.len().saturating_sub(visible_count);
            let visible_items: Vec<ListItem> = items.into_iter().skip(skip).collect();
            f.render_widget(
                List::new(visible_items),
                ratatui::layout::Rect {
                    y: list_area.y + list_area.height.saturating_sub(visible_count as u16),
                    height: visible_count as u16,
                    ..list_area
                },
            );

            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {} notification(s)", count),
                    Style::default().fg(Color::Cyan),
                ))),
                chunks[2],
            );

            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "enter: jump | d: delete | esc: close",
                    Style::default().fg(Color::Green),
                ))),
                chunks[3],
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
                (_, KeyCode::Up) | (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                    let len = notifs.len();
                    if len > 0 {
                        selected = (selected + len - 1) % len;
                    }
                }
                (_, KeyCode::Down) | (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
                    let len = notifs.len();
                    if len > 0 {
                        selected = (selected + 1) % len;
                    }
                }
                _ => {}
            }
        }
    }
}
