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

const REFRESH_INTERVAL: Duration = Duration::from_millis(100);
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
    let active_windows: std::collections::HashSet<String> =
        crate::tmux::active_windows().into_iter().collect();

    let mut workers: Vec<Worker> = crate::status::list()
        .into_iter()
        .filter_map(|path| crate::status::read(&path))
        .filter_map(|mut entry| {
            // Decay "done" → "idle" once the user has actually focused the window.
            // This makes blue act as a notification badge that fades after viewing,
            // instead of every-finished-Claude staying blue forever.
            if entry.status == "done" && active_windows.contains(&entry.pane) {
                let _ = crate::status::write(&entry.pane, "idle");
                entry.status = "idle".to_string();
            }
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

    // Stable order so a user-referenced index ("agent 3") doesn't shuffle as
    // workers flip between idle/working.
    workers.sort_by(|a, b| a.pane.cmp(&b.pane));

    workers
}

#[cfg_attr(not(feature = "voice"), allow(dead_code))]
pub fn ordered_panes() -> Vec<String> {
    let live: std::collections::HashSet<String> =
        crate::tmux::list_sessions().into_iter().collect();
    let mut panes: Vec<String> = crate::status::list()
        .into_iter()
        .filter_map(|p| crate::status::read(&p))
        .filter_map(|e| {
            let session = e.pane.split(':').next()?.to_string();
            if live.contains(&session) {
                Some(e.pane)
            } else {
                None
            }
        })
        .collect();
    panes.sort();
    panes
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

pub fn run_panel(config: Config) -> Result<()> {
    // Tag this pane so the sweep / hook installer can recognize it and avoid
    // creating duplicate panels in the same window.
    let _ = crate::tmux::set_current_pane_title(crate::tmux::PANEL_PANE_TITLE);

    terminal::enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let result = run_panel_loop(&mut terminal, &config);
    terminal::disable_raw_mode()?;
    result
}

fn run_panel_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: &Config,
) -> Result<()> {
    let mut selected: Option<usize> = None;

    loop {
        let workers = collect_workers(config);
        if let Some(i) = selected {
            if workers.is_empty() {
                selected = None;
            } else if i >= workers.len() {
                selected = Some(workers.len() - 1);
            }
        }
        if terminal.draw(|f| draw(f, &workers, selected)).is_err() {
            return Ok(());
        }

        if event::poll(REFRESH_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                let len = workers.len();
                match (key.modifiers, key.code) {
                    // Ctrl-j / Down: move selection down (start at 0 if unselected)
                    (KeyModifiers::CONTROL, KeyCode::Char('j')) | (_, KeyCode::Down) => {
                        if len > 0 {
                            selected = Some(match selected {
                                None => 0,
                                Some(i) => (i + 1) % len,
                            });
                        }
                    }
                    // Ctrl-k / Up: move selection up
                    (KeyModifiers::CONTROL, KeyCode::Char('k')) | (_, KeyCode::Up) => {
                        if len > 0 {
                            selected = Some(match selected {
                                None => len - 1,
                                Some(i) => (i + len - 1) % len,
                            });
                        }
                    }
                    // Enter: jump to the selected worker (only if one is selected)
                    (_, KeyCode::Enter) => {
                        if let Some(i) = selected {
                            if let Some(w) = workers.get(i) {
                                if let Some((session, window)) = w.pane.rsplit_once(':') {
                                    let _ = crate::tmux::switch_client(session);
                                    let _ = std::process::Command::new("tmux")
                                        .args([
                                            "select-window",
                                            "-t",
                                            &format!("{session}:{window}"),
                                        ])
                                        .status();
                                }
                                // Clear selection so the next focus starts fresh.
                                selected = None;
                            }
                        }
                    }
                    // All other keys (Esc, q, Ctrl-c, ...) are ignored — exiting
                    // would close the tmux pane.
                    _ => {}
                }
            }
        }
    }
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
        if terminal.draw(|f| draw(f, &workers, Some(selected))).is_err() {
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

fn draw(f: &mut ratatui::Frame, workers: &[Worker], selected: Option<usize>) {
    let area = f.area();
    let active_windows: std::collections::HashSet<String> =
        crate::tmux::active_windows().into_iter().collect();

    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .split(area);

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
        let is_active_window = active_windows.contains(&w.pane);
        draw_tile(
            f,
            Rect::new(body_x, y, body_w, tile_h),
            w,
            i + 1,
            selected == Some(i),
            is_active_window,
        );
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

    if area.width >= 2 && area.height >= 1 {
        draw_voice_status(f, area);
    }
}

fn draw_voice_status(f: &mut ratatui::Frame, area: Rect) {
    let recording = std::path::Path::new(crate::VOICE_RECORDING_FLAG).exists();
    let transcribing = std::path::Path::new(crate::VOICE_TRANSCRIBING_FLAG).exists();
    let label = if transcribing {
        Some("transcribing…".to_string())
    } else {
        read_recent_last_command()
    };
    let show_dot = recording && !transcribing;

    if !show_dot && label.is_none() {
        return;
    }

    let row = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    // Reserved unconditionally so the label doesn't reflow when the dot lights up.
    let dot_slot: u16 = 2;
    let right_pad: u16 = 1;
    let total_w = row.width;
    let label_w = total_w.saturating_sub(dot_slot + right_pad);

    if let Some(label) = label.as_deref() {
        let truncated = truncate_to_cells(label, label_w as usize);
        let label_rect = Rect::new(row.x, row.y, label_w, 1);
        let line = Line::from(Span::styled(
            truncated,
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(ratatui::layout::Alignment::Left);
        f.render_widget(Paragraph::new(line), label_rect);
    }

    if show_dot {
        let dot_rect = Rect::new(
            row.x + total_w.saturating_sub(dot_slot + right_pad),
            row.y,
            dot_slot,
            1,
        );
        let line = Line::from(Span::styled(
            " ●",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ));
        f.render_widget(Paragraph::new(line), dot_rect);
    }
}

fn truncate_to_cells(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

fn read_recent_last_command() -> Option<String> {
    const MAX_AGE: Duration = Duration::from_secs(5);
    let path = crate::VOICE_LAST_COMMAND_FILE;
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    if mtime.elapsed().ok()? > MAX_AGE {
        return None;
    }
    let s = std::fs::read_to_string(path).ok()?;
    let s = s.trim();
    if s.is_empty() { None } else { Some(s.to_string()) }
}

fn draw_tile(
    f: &mut ratatui::Frame,
    area: Rect,
    w: &Worker,
    index: usize,
    selected: bool,
    is_active_window: bool,
) {
    // Idle is the only state we dim — Working/Waiting/Done all keep their accent
    // so the user can distinguish "completed" (blue) from "nothing happening" (gray).
    let prominent = !matches!(w.state, WorkerState::Idle) || w.task_message.is_some();
    let accent = w.state.color();
    let base_border = if prominent { accent } else { INACTIVE_BORDER };
    // Selection (orange) > active window (magenta) > state accent.
    let border_color = if selected {
        Color::Rgb(0xff, 0x8c, 0x00)
    } else if is_active_window {
        Color::Magenta
    } else {
        base_border
    };
    let title_color = if prominent || is_active_window {
        Color::White
    } else {
        INACTIVE_TEXT
    };
    let body_color = if prominent { Color::Gray } else { INACTIVE_DIM };
    let task_color = if prominent { accent } else { INACTIVE_DIM };

    let border_style = if selected || is_active_window {
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
    let idx_str = format!("{index}.");
    let prefix_len = idx_str.chars().count() + 1 + 2; // "N. " + "X "
    let feature = truncate(
        &w.feature,
        inner
            .width
            .saturating_sub(prefix_len as u16 + work.chars().count() as u16 + 1) as usize,
    );
    let pad = (inner.width as usize)
        .saturating_sub(prefix_len + feature.chars().count() + work.chars().count());
    let line1 = Line::from(vec![
        Span::styled(
            format!("{idx_str} "),
            Style::default().fg(INACTIVE_TEXT).add_modifier(Modifier::BOLD),
        ),
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

