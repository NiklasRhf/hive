use crate::config::Config;
use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use std::collections::HashMap;
use std::io;
use std::time::Duration;

const REFRESH_INTERVAL: Duration = Duration::from_millis(1000);
const INACTIVE_BORDER: Color = Color::Rgb(0x6b, 0x72, 0x80);
const INACTIVE_TEXT: Color = Color::Rgb(0x9c, 0xa3, 0xaf);
const INACTIVE_DIM: Color = Color::Rgb(0x6b, 0x72, 0x80);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerState {
    Working,
    Waiting,
    Done,
    Idle,
}

impl WorkerState {
    fn is_active(self) -> bool {
        matches!(self, WorkerState::Working | WorkerState::Waiting)
    }

    fn icon(self) -> &'static str {
        match self {
            WorkerState::Working => ">",
            WorkerState::Waiting => "!",
            WorkerState::Done => "*",
            WorkerState::Idle => "-",
        }
    }

    fn color(self) -> Color {
        match self {
            WorkerState::Working => Color::Green,
            WorkerState::Waiting => Color::Yellow,
            WorkerState::Done => Color::Blue,
            WorkerState::Idle => Color::DarkGray,
        }
    }

    fn label(self) -> &'static str {
        match self {
            WorkerState::Working => "working",
            WorkerState::Waiting => "waiting for input",
            WorkerState::Done => "done",
            WorkerState::Idle => "idle",
        }
    }
}

#[derive(Debug, Clone)]
struct Worker {
    pane: String,
    project: String,
    feature: String,
    state: WorkerState,
    last_activity: Option<DateTime<Utc>>,
    work_secs: i64,
    task_message: Option<String>,
}

#[derive(serde::Deserialize)]
struct HistoryEvent {
    ts: DateTime<Utc>,
    pane: String,
    status: String,
}

#[derive(serde::Deserialize)]
struct NotifEntry {
    pane: String,
    message: String,
}

fn history_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("hive")
        .join("history.jsonl")
}

fn load_history() -> Vec<HistoryEvent> {
    let Ok(content) = std::fs::read_to_string(history_path()) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|l| serde_json::from_str::<HistoryEvent>(l).ok())
        .collect()
}

fn load_notifications() -> HashMap<String, String> {
    let Ok(content) = std::fs::read_to_string(crate::NOTIF_FILE) else {
        return HashMap::new();
    };
    let entries: Vec<NotifEntry> = serde_json::from_str(&content).unwrap_or_default();
    entries.into_iter().map(|n| (n.pane, n.message)).collect()
}

fn compute_work_secs(history: &[HistoryEvent]) -> HashMap<String, i64> {
    let mut by_pane: HashMap<String, Vec<&HistoryEvent>> = HashMap::new();
    for e in history {
        by_pane.entry(e.pane.clone()).or_default().push(e);
    }
    let mut out = HashMap::new();
    for (pane, evts) in by_pane {
        let mut secs: i64 = 0;
        let mut start: Option<DateTime<Utc>> = None;
        for e in evts {
            match e.status.as_str() {
                "working" => {
                    if start.is_none() {
                        start = Some(e.ts);
                    }
                }
                "waiting" | "done" | "idle_done" => {
                    if let Some(s) = start.take() {
                        secs += (e.ts - s).num_seconds().max(0);
                    }
                }
                _ => {}
            }
        }
        out.insert(pane, secs);
    }
    out
}

fn derive_project_and_feature(session: &str, config: &Config) -> (String, String) {
    if config.find_project(session).is_some() {
        return (session.to_string(), "main".to_string());
    }
    if let Some((parent, rest)) = session.split_once('-') {
        if config.find_project(parent).is_some() {
            return (parent.to_string(), rest.to_string());
        }
    }
    (session.to_string(), session.to_string())
}

fn collect_workers(config: &Config) -> Vec<Worker> {
    let history = load_history();
    let work_secs = compute_work_secs(&history);
    let notifs = load_notifications();
    let live_sessions: std::collections::HashSet<String> =
        crate::tmux::list_sessions().into_iter().collect();

    let mut workers: Vec<Worker> = crate::status::list()
        .into_iter()
        .filter_map(|path| crate::status::read(&path))
        .filter_map(|entry| {
            let pane = entry.pane.clone();
            let session = pane.split(':').next()?.to_string();
            if !live_sessions.contains(&session) {
                return None;
            }
            let state = match entry.status.as_str() {
                "working" => WorkerState::Working,
                "waiting" => WorkerState::Waiting,
                "done" => WorkerState::Done,
                _ => WorkerState::Idle,
            };
            let last_activity = DateTime::<Utc>::from_timestamp(entry.ts, 0);
            let (project, feature) = derive_project_and_feature(&session, config);
            Some(Worker {
                pane: pane.clone(),
                project,
                feature,
                state,
                last_activity,
                work_secs: work_secs.get(&pane).copied().unwrap_or(0),
                task_message: notifs.get(&pane).cloned(),
            })
        })
        .collect();

    workers.sort_by(|a, b| {
        let a_active = a.state.is_active();
        let b_active = b.state.is_active();
        b_active.cmp(&a_active).then_with(|| {
            b.last_activity
                .unwrap_or(DateTime::<Utc>::MIN_UTC)
                .cmp(&a.last_activity.unwrap_or(DateTime::<Utc>::MIN_UTC))
        })
    });

    workers
}

fn format_elapsed(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn elapsed_since(ts: DateTime<Utc>) -> String {
    let secs = (Utc::now() - ts).num_seconds().max(0);
    format_elapsed(secs)
}

pub fn run(config: Config) -> Result<()> {
    terminal::enable_raw_mode()?;
    while event::poll(Duration::from_millis(10))? {
        let _ = event::read()?;
    }
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let toggle_key = parse_keybind(&config.keybindings.dock);
    let result = run_loop(&mut terminal, &config, toggle_key);
    terminal::disable_raw_mode()?;
    result
}

fn parse_keybind(s: &str) -> Option<(KeyModifiers, KeyCode)> {
    let mut mods = KeyModifiers::empty();
    let mut rest = s;
    loop {
        if let Some(r) = rest.strip_prefix("M-") {
            mods |= KeyModifiers::ALT;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("C-") {
            mods |= KeyModifiers::CONTROL;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("S-") {
            mods |= KeyModifiers::SHIFT;
            rest = r;
        } else {
            break;
        }
    }
    let code = if rest.chars().count() == 1 {
        KeyCode::Char(rest.chars().next().unwrap())
    } else {
        return None;
    };
    Some((mods, code))
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: &Config,
    toggle_key: Option<(KeyModifiers, KeyCode)>,
) -> Result<()> {
    let mut selected: usize = 0;

    loop {
        let workers = collect_workers(config);
        if !workers.is_empty() && selected >= workers.len() {
            selected = workers.len() - 1;
        }
        if terminal.draw(|f| draw(f, &workers, selected)).is_err() {
            return Ok(());
        }

        if event::poll(REFRESH_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                match (key.modifiers, key.code) {
                    (_, KeyCode::Esc)
                    | (_, KeyCode::Char('q'))
                    | (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(()),
                    (m, c) if Some((m, c)) == toggle_key => return Ok(()),
                    (_, KeyCode::Up) | (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                        let len = workers.len();
                        if len > 0 {
                            selected = (selected + len - 1) % len;
                        }
                    }
                    (_, KeyCode::Down) | (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
                        let len = workers.len();
                        if len > 0 {
                            selected = (selected + 1) % len;
                        }
                    }
                    (_, KeyCode::Enter) => {
                        if let Some(w) = workers.get(selected) {
                            if let Some((session, window)) = w.pane.rsplit_once(':') {
                                let _ = crate::tmux::switch_client(session);
                                let _ = std::process::Command::new("tmux")
                                    .args(["select-window", "-t", &format!("{session}:{window}")])
                                    .status();
                            }
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn draw(f: &mut ratatui::Frame, workers: &[Worker], selected: usize) {
    let area = f.area();
    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(area);

    let active = workers.iter().filter(|w| w.state.is_active()).count();
    let total = workers.len();
    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            " HIVE",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {active} active · {total} total"),
            Style::default().fg(Color::Gray),
        )),
    ]);
    f.render_widget(header, chunks[0]);

    let mut y = chunks[1].y;
    let max_y = chunks[1].y + chunks[1].height;
    let body_x = chunks[1].x;
    let body_w = chunks[1].width;

    for (i, w) in workers.iter().enumerate() {
        let tile_h: u16 = 5;
        if y + tile_h > max_y {
            break;
        }
        draw_tile(f, Rect::new(body_x, y, body_w, tile_h), w, i == selected);
        y += tile_h;
    }

    if workers.is_empty() && chunks[1].height >= 1 {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " no claude workers",
                Style::default().fg(Color::Gray),
            ))),
            Rect::new(body_x, chunks[1].y + 1, body_w, 1),
        );
    }
}

fn draw_tile(f: &mut ratatui::Frame, area: Rect, w: &Worker, selected: bool) {
    let needs_attention = w.state.is_active() || w.task_message.is_some();
    let accent = w.state.color();
    let base_border = if needs_attention { accent } else { INACTIVE_BORDER };
    let border_color = if selected {
        Color::Rgb(0xff, 0x8c, 0x00)
    } else {
        base_border
    };
    let title_color = if needs_attention { Color::White } else { INACTIVE_TEXT };
    let body_color = if needs_attention { Color::Gray } else { INACTIVE_DIM };
    let task_color = if needs_attention { accent } else { INACTIVE_DIM };

    let border_style = if selected {
        Style::default().fg(border_color).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(border_color)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 1 || inner.width < 4 {
        return;
    }

    let work = if w.work_secs > 0 {
        format_elapsed(w.work_secs)
    } else {
        "—".to_string()
    };
    let icon = w.state.icon();
    let feature = truncate(
        &w.feature,
        inner
            .width
            .saturating_sub(2 + work.chars().count() as u16 + 1) as usize,
    );
    let pad = (inner.width as usize)
        .saturating_sub(2 + feature.chars().count() + work.chars().count());
    let line1 = Line::from(vec![
        Span::styled(
            format!("{icon} "),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            feature,
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(pad)),
        Span::styled(work, Style::default().fg(accent)),
    ]);
    f.render_widget(
        Paragraph::new(line1),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height >= 2 {
        let activity = w
            .last_activity
            .map(|t| format!("{} ago", elapsed_since(t)))
            .unwrap_or_default();
        let proj = truncate(
            &w.project,
            inner
                .width
                .saturating_sub(2 + activity.chars().count() as u16 + 1) as usize,
        );
        let pad2 = (inner.width as usize)
            .saturating_sub(2 + proj.chars().count() + activity.chars().count());
        let line2 = Line::from(vec![
            Span::raw("  "),
            Span::styled(proj, Style::default().fg(body_color)),
            Span::raw(" ".repeat(pad2)),
            Span::styled(activity, Style::default().fg(body_color)),
        ]);
        f.render_widget(
            Paragraph::new(line2),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }

    if inner.height >= 3 {
        let task = w
            .task_message
            .clone()
            .unwrap_or_else(|| w.state.label().to_string());
        let line3 = Line::from(vec![
            Span::raw("  "),
            Span::styled(
                truncate(&task, inner.width.saturating_sub(2) as usize),
                Style::default().fg(task_color),
            ),
        ]);
        f.render_widget(
            Paragraph::new(line3),
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

