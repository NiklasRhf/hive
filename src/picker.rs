use crate::config::Config;
use crate::session::{self, SessionEntry, Status};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use nucleo::{Config as NucleoConfig, Nucleo};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, List, ListItem, Paragraph};
use std::io;
use std::sync::Arc;

#[derive(PartialEq)]
enum Mode {
    All,
    WorktreesOnly,
}

enum PickerResult {
    Open(String),
    SwitchTo(String),
    Quit,
}

enum View {
    List,
    NewProjectName(String),
    NewBranchName(String, String),
}

enum Action {
    Open(String),
    Kill(String),
    NewSession,
    ToggleMode,
    Quit,
}

struct Picker {
    input: String,
    sessions: Vec<SessionEntry>,
    selected: usize,
    mode: Mode,
    matcher: Nucleo<usize>,
    filtered: Vec<usize>,
    view: View,
}

impl Picker {
    fn new(sessions: Vec<SessionEntry>) -> Self {
        let matcher = Nucleo::new(NucleoConfig::DEFAULT, Arc::new(|| {}), None, 1);
        let mut picker = Self {
            input: String::new(),
            sessions,
            selected: 0,
            mode: Mode::All,
            matcher,
            filtered: Vec::new(),
            view: View::List,
        };
        picker.inject_items();
        picker.refresh_filtered();
        picker
    }

    fn inject_items(&mut self) {
        self.matcher.restart(false);
        let injector = self.matcher.injector();
        for (i, entry) in self.sessions.iter().enumerate() {
            if self.mode == Mode::WorktreesOnly && entry.status != Status::Worktree {
                continue;
            }
            let _ = injector.push(i, |_, cols| {
                cols[0] = entry.name.as_str().into();
            });
        }
    }

    fn update_pattern(&mut self) {
        self.matcher.pattern.reparse(
            0,
            &self.input,
            nucleo::pattern::CaseMatching::Smart,
            nucleo::pattern::Normalization::Smart,
            false,
        );
        self.refresh_filtered();
    }

    fn refresh_filtered(&mut self) {
        self.matcher.tick(10);
        let snapshot = self.matcher.snapshot();
        let mut indices: Vec<usize> = snapshot
            .matched_items(..snapshot.matched_item_count())
            .map(|item| *item.data)
            .collect();
        indices.sort_by(|&a, &b| {
            let ea = &self.sessions[a];
            let eb = &self.sessions[b];
            let ord = |s: &Status| match s {
                Status::Stopped => 0,
                Status::Worktree => 1,
                Status::Running => 2,
            };
            ord(&ea.status)
                .cmp(&ord(&eb.status))
                .then(ea.name.cmp(&eb.name))
        });
        self.filtered = indices;
    }

    fn selected_name(&self) -> Option<String> {
        self.filtered
            .get(self.selected)
            .map(|&i| self.sessions[i].name.clone())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::Esc) => Some(Action::Quit),
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => self.selected_name().map(Action::Kill),
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => Some(Action::NewSession),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => Some(Action::ToggleMode),
            (_, KeyCode::Enter) => self.selected_name().map(Action::Open),
            (_, KeyCode::Up) | (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                let len = self.filtered.len();
                if len > 0 {
                    self.selected = (self.selected + len - 1) % len;
                }
                None
            }
            (_, KeyCode::Down) | (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
                let len = self.filtered.len();
                if len > 0 {
                    self.selected = (self.selected + 1) % len;
                }
                None
            }
            (_, KeyCode::Backspace) => {
                self.input.pop();
                self.update_pattern();
                self.selected = 0;
                None
            }
            (_, KeyCode::Char(c)) => {
                self.input.push(c);
                self.update_pattern();
                self.selected = 0;
                None
            }
            _ => None,
        }
    }
}

fn draw(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, picker: &Picker) -> Result<()> {
    terminal.draw(|f| {
        let area = f.area();
        match &picker.view {
            View::List => draw_list_view(f, area, picker),
            View::NewProjectName(input) => draw_input_view(f, area, "Project name:", input),
            View::NewBranchName(project, input) => {
                draw_input_view(f, area, &format!("Branch for {project} (optional):"), input)
            }
        }
    })?;
    Ok(())
}

const LOGO: [&str; 3] = [
    "█ █  █  █   █  ████",
    "███  █   █ █   █▄▄ ",
    "█ █  █    █    ████",
];

fn draw_logo(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let lines: Vec<Line> = LOGO
        .iter()
        .map(|l| Line::from(Span::styled(*l, Style::default().fg(Color::Cyan))))
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_list_view(f: &mut ratatui::Frame, area: ratatui::layout::Rect, picker: &Picker) {
    let header_text = match picker.mode {
        Mode::All => "enter: open | ctrl-d: kill | ctrl-n: new | ctrl-w: worktrees",
        Mode::WorktreesOnly => "enter: open | ctrl-d: kill+rm | ctrl-w: all",
    };

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    draw_logo(f, chunks[0]);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            header_text,
            Style::default().fg(Color::Green),
        ))),
        chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(&picker.input),
        ])),
        chunks[2],
    );

    let items: Vec<ListItem> = picker
        .filtered
        .iter()
        .enumerate()
        .map(|(i, &idx)| {
            let entry = &picker.sessions[idx];
            let icon_color = match entry.status {
                Status::Running => Color::Green,
                Status::Stopped => Color::White,
                Status::Worktree => Color::Yellow,
            };
            let style = if i == picker.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", entry.icon()),
                    Style::default().fg(icon_color),
                ),
                Span::styled(entry.name.clone(), style),
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
}

fn draw_input_view(f: &mut ratatui::Frame, area: ratatui::layout::Rect, label: &str, input: &str) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    draw_logo(f, chunks[0]);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label,
            Style::default().fg(Color::White),
        ))),
        chunks[2],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(input),
        ])),
        chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "esc: back | enter: confirm",
            Style::default().fg(Color::Green),
        ))),
        chunks[4],
    );
}

fn draw_progress(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    title: &str,
    ratio: f64,
) -> Result<()> {
    terminal.draw(|f| {
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(f.area());

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                title,
                Style::default().fg(Color::White),
            ))),
            chunks[1],
        );

        f.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(Color::Cyan))
                .ratio(ratio.clamp(0.0, 1.0))
                .label(Span::styled(
                    format!("{}%", (ratio * 100.0).round() as u8),
                    Style::default()
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )),
            chunks[2],
        );
    })?;
    Ok(())
}

fn run_picker(config: &Config, use_alt_screen: bool) -> Result<Option<PickerResult>> {
    terminal::enable_raw_mode()?;

    while crossterm::event::poll(std::time::Duration::from_millis(50))? {
        let _ = crossterm::event::read()?;
    }

    let mut stdout = io::stdout();
    if use_alt_screen {
        crossterm::execute!(stdout, EnterAlternateScreen)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let sessions = session::discover(config);
    let mut picker = Picker::new(sessions);
    let result = picker_loop(&mut terminal, &mut picker, config);

    terminal::disable_raw_mode()?;
    if use_alt_screen {
        crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }

    result
}

fn picker_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    picker: &mut Picker,
    config: &Config,
) -> Result<Option<PickerResult>> {
    loop {
        draw(terminal, picker)?;

        if let Event::Key(key) = event::read()? {
            match &mut picker.view {
                View::List => match picker.handle_key(key) {
                    Some(Action::Quit) => return Ok(Some(PickerResult::Quit)),
                    Some(Action::Open(name)) => return Ok(Some(PickerResult::Open(name))),
                    Some(Action::Kill(name)) => {
                        let _ = crate::tmux::kill_session(&name);
                        if picker.mode == Mode::WorktreesOnly {
                            let parent = name.split('-').next().unwrap_or(&name);
                            if let Some(project) = config.find_project(parent) {
                                let _ = crate::worktree::remove(project, &name);
                            }
                        }
                        picker.sessions = session::discover(config);
                        picker.selected = 0;
                        picker.inject_items();
                        picker.update_pattern();
                    }
                    Some(Action::NewSession) => {
                        picker.view = View::NewProjectName(String::new());
                    }
                    Some(Action::ToggleMode) => {
                        picker.mode = match picker.mode {
                            Mode::All => Mode::WorktreesOnly,
                            Mode::WorktreesOnly => Mode::All,
                        };
                        picker.selected = 0;
                        picker.inject_items();
                        picker.update_pattern();
                    }
                    None => {}
                },
                View::NewProjectName(input) => match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::Esc) => {
                        picker.view = View::List;
                    }
                    (_, KeyCode::Enter) => {
                        let name = input.trim().to_string();
                        if name.is_empty() {
                            picker.view = View::List;
                        } else if config.find_project(&name).is_some() {
                            picker.view = View::NewBranchName(name, String::new());
                        } else {
                            let home = dirs::home_dir()
                                .map(|h| h.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "~".to_string());
                            crate::tmux::create_blank_session(&name, &home)?;
                            return Ok(Some(PickerResult::SwitchTo(name)));
                        }
                    }
                    (_, KeyCode::Backspace) => {
                        input.pop();
                    }
                    (_, KeyCode::Char(c)) => {
                        input.push(c);
                    }
                    _ => {}
                },
                View::NewBranchName(project_name, input) => match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::Esc) => {
                        picker.view = View::List;
                    }
                    (_, KeyCode::Enter) => {
                        let name = project_name.clone();
                        let branch = input.trim().to_string();
                        let session_name =
                            create_new_session_with_progress(terminal, &name, &branch, config)?;
                        return Ok(Some(PickerResult::SwitchTo(session_name)));
                    }
                    (_, KeyCode::Backspace) => {
                        input.pop();
                    }
                    (_, KeyCode::Char(c)) => {
                        input.push(c);
                    }
                    _ => {}
                },
            }
        }
    }
}

fn create_and_open_session(name: &str, config: &Config) -> Result<()> {
    if !crate::tmux::has_session(name) {
        if let Some(path) = session::resolve_path(name, config) {
            let cmd = session::resolve_cmd(name, config);
            crate::tmux::create_project_session(name, &path.to_string_lossy(), &cmd)?;
        }
    }
    Ok(())
}

fn animate_progress(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    title: &str,
    target: &Arc<std::sync::Mutex<f64>>,
    done: &Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    let mut current = 0.0f64;
    let frame_dur = std::time::Duration::from_millis(16);

    while !done.load(Ordering::Relaxed) || current < 0.999 {
        let t = *target.lock().unwrap();
        let goal = if done.load(Ordering::Relaxed) { 1.0 } else { t };
        current += (goal - current) * 0.08;
        if (goal - current).abs() < 0.005 {
            current = goal;
        }
        let _ = draw_progress(terminal, title, current);
        std::thread::sleep(frame_dur);
    }

    let _ = draw_progress(terminal, title, 1.0);
    std::thread::sleep(std::time::Duration::from_secs(1));
}

fn create_new_session_with_progress(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    name: &str,
    branch: &str,
    config: &Config,
) -> Result<String> {
    let session_name = if branch.is_empty() {
        name.to_string()
    } else {
        format!("{name}-{branch}")
    };

    let project = config.find_project(name);

    let path = if let Some(p) = project {
        if !branch.is_empty() && p.worktree.is_some() {
            let target = Arc::new(std::sync::Mutex::new(0.0f64));
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let target_clone = target.clone();
            let done_clone = done.clone();
            let pc = p.clone();
            let b = branch.to_string();

            let handle = std::thread::spawn(move || {
                let result = crate::worktree::create(&pc, &b, &mut |prog| {
                    *target_clone.lock().unwrap() = prog.ratio;
                });
                done_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                result
            });

            let title = format!("Creating worktree for {session_name}");
            animate_progress(terminal, &title, &target, &done);

            handle.join().unwrap()?.to_string_lossy().into_owned()
        } else {
            p.path.clone()
        }
    } else {
        "~".to_string()
    };

    let cmd = project
        .and_then(|p| p.cmd.as_deref())
        .unwrap_or("git status");
    crate::tmux::create_project_session(&session_name, &path, cmd)?;
    Ok(session_name)
}

pub fn run(config: Config) -> Result<()> {
    match run_picker(&config, false)? {
        Some(PickerResult::Open(name)) => {
            create_and_open_session(&name, &config)?;
            crate::tmux::switch_client(&name)?;
        }
        Some(PickerResult::SwitchTo(name)) => {
            crate::tmux::switch_client(&name)?;
        }
        Some(PickerResult::Quit) | None => {}
    }
    Ok(())
}

pub fn run_and_return(config: &Config) -> Result<Option<String>> {
    match run_picker(config, true)? {
        Some(PickerResult::Open(name)) => {
            create_and_open_session(&name, config)?;
            Ok(Some(name))
        }
        Some(PickerResult::SwitchTo(name)) => Ok(Some(name)),
        Some(PickerResult::Quit) | None => Ok(None),
    }
}
