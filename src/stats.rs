use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, Utc};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Paragraph};
use std::collections::HashMap;
use std::io;

#[derive(serde::Deserialize)]
struct Event_ {
    ts: DateTime<Utc>,
    pane: String,
    status: String,
}

#[derive(PartialEq)]
enum Tab {
    Today,
    Week,
}

fn history_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("hive")
        .join("history.jsonl")
}

fn load_events() -> Result<Vec<Event_>> {
    let content = std::fs::read_to_string(history_path())
        .context("no history file — has the watcher run yet?")?;
    let mut events = Vec::new();
    for line in content.lines() {
        if let Ok(e) = serde_json::from_str::<Event_>(line) {
            events.push(e);
        }
    }
    Ok(events)
}

struct SessionStats {
    name: String,
    work_secs: i64,
    events: usize,
}

fn compute_stats(events: &[Event_], since: DateTime<Utc>) -> Vec<SessionStats> {
    let filtered: Vec<&Event_> = events.iter().filter(|e| e.ts >= since).collect();

    let mut by_session: HashMap<String, Vec<&Event_>> = HashMap::new();
    for e in &filtered {
        let session = e.pane.rsplit_once(':').map(|(s, _)| s).unwrap_or(&e.pane);
        by_session.entry(session.to_string()).or_default().push(e);
    }

    let mut stats: Vec<SessionStats> = by_session
        .into_iter()
        .map(|(name, evts)| {
            let mut work_secs: i64 = 0;
            let mut last_working: Option<DateTime<Utc>> = None;

            for e in &evts {
                match e.status.as_str() {
                    "working" => {
                        if last_working.is_none() {
                            last_working = Some(e.ts);
                        }
                    }
                    "done" | "idle_done" | "waiting" => {
                        if let Some(start) = last_working.take() {
                            work_secs += (e.ts - start).num_seconds().max(0);
                        }
                    }
                    _ => {}
                }
            }

            SessionStats {
                name,
                work_secs,
                events: evts.len(),
            }
        })
        .collect();

    stats.sort_by(|a, b| b.work_secs.cmp(&a.work_secs));
    stats
}

fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub fn run() -> Result<()> {
    let events = load_events()?;
    if events.is_empty() {
        println!("No history data yet.");
        return Ok(());
    }

    terminal::enable_raw_mode()?;
    while event::poll(std::time::Duration::from_millis(50))? {
        let _ = event::read()?;
    }

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, &events);
    terminal::disable_raw_mode()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    events: &[Event_],
) -> Result<()> {
    let mut tab = Tab::Today;

    loop {
        let since = match tab {
            Tab::Today => Local::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap()
                .with_timezone(&Utc),
            Tab::Week => Utc::now() - Duration::days(7),
        };

        let stats = compute_stats(events, since);

        terminal.draw(|f| {
            let chunks = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Min(4),
                Constraint::Length(1),
            ])
            .split(f.area());

            const LOGO: [&str; 3] = [
                "█ █  █  █   █  ████",
                "███  █   █ █   █▄▄ ",
                "█ █  █    █    ████",
            ];
            let logo_lines: Vec<Line> = LOGO
                .iter()
                .map(|l| Line::from(Span::styled(*l, Style::default().fg(Color::Cyan))))
                .collect();
            f.render_widget(Paragraph::new(logo_lines), chunks[0]);

            let tab_line = Line::from(vec![
                if tab == Tab::Today {
                    Span::styled(
                        " [today] ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("  today  ", Style::default().fg(Color::White))
                },
                Span::raw(" "),
                if tab == Tab::Week {
                    Span::styled(
                        " [7 days] ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("  7 days  ", Style::default().fg(Color::White))
                },
            ]);
            f.render_widget(Paragraph::new(tab_line), chunks[1]);

            if stats.is_empty() {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "No activity",
                        Style::default().fg(Color::White),
                    ))),
                    chunks[2],
                );
            } else {
                const PALETTE: &[Color] = &[
                    Color::Cyan,
                    Color::Green,
                    Color::Yellow,
                    Color::Magenta,
                    Color::Blue,
                    Color::LightCyan,
                    Color::LightGreen,
                    Color::LightYellow,
                    Color::LightMagenta,
                    Color::LightBlue,
                    Color::Red,
                ];

                let max_secs = stats.iter().map(|s| s.work_secs).max().unwrap_or(1).max(1) as u64;
                let bars: Vec<Bar> = stats
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let color = PALETTE[i % PALETTE.len()];
                        Bar::default()
                            .label(Line::from(s.name.as_str()))
                            .value(s.work_secs.max(0) as u64)
                            .style(Style::default().fg(color))
                    })
                    .collect();

                let bar_chart = BarChart::default()
                    .data(BarGroup::default().bars(&bars))
                    .bar_width(if !stats.is_empty() {
                        ((chunks[2].width as usize / stats.len()).max(3).min(12)) as u16
                    } else {
                        5
                    })
                    .bar_gap(1)
                    .value_style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    )
                    .label_style(Style::default().fg(Color::White))
                    .max(max_secs);

                f.render_widget(bar_chart, chunks[2]);
            }

            let total_work: i64 = stats.iter().map(|s| s.work_secs).sum();
            let total_events: usize = stats.iter().map(|s| s.events).sum();
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!(
                            "total: {} | {} events | ",
                            format_duration(total_work),
                            total_events
                        ),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        "tab: switch | esc: close",
                        Style::default().fg(Color::Green),
                    ),
                ])),
                chunks[3],
            );
        })?;

        if let Event::Key(key) = event::read()? {
            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(()),
                (_, KeyCode::Tab) => {
                    tab = match tab {
                        Tab::Today => Tab::Week,
                        Tab::Week => Tab::Today,
                    };
                }
                _ => {}
            }
        }
    }
}
