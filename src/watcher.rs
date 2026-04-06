use crate::config::Config;
use anyhow::{Context, Result};
use cairo::{Format, ImageSurface};
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::shape;
use x11rb::protocol::xproto::*;

const STATUS_DIR: &str = "/tmp";
const STATUS_PREFIX: &str = "claude-status-";

const CARD_PADDING: f64 = 10.0;
const CARD_MARGIN: f64 = 4.0;
const CARD_ROUNDING: f64 = 6.0;
const FONT_SIZE: f64 = 13.0;
const LINE_HEIGHT: f64 = 18.0;
const CARD_VERT_PADDING: f64 = 10.0;
const MAX_VISIBLE: usize = 8;
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const NOTIF_MARGIN: i16 = 10;

#[derive(Debug, Clone)]
struct PaneState {
    status: PaneStatus,
    last_activity: Instant,
}

#[derive(Debug, Clone, PartialEq)]
enum PaneStatus {
    Idle,
    Running,
    Waiting,
    Done,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Notification {
    pane: String,
    message: String,
}

type SharedState = Arc<Mutex<WatcherState>>;

#[derive(Debug)]
struct WatcherState {
    pane_states: HashMap<String, PaneState>,
    notifications: Vec<Notification>,
}

impl WatcherState {
    fn new() -> Self {
        Self {
            pane_states: HashMap::new(),
            notifications: Vec::new(),
        }
    }

    fn dismiss_visited(&mut self) {
        let active = crate::tmux::active_windows();
        if active.is_empty() {
            return;
        }
        let before = self.notifications.len();
        self.notifications.retain(|n| !active.contains(&n.pane));
        if self.notifications.len() != before {
            eprintln!("watcher: dismissed notifications for {:?} (visited)", active);
        }
    }

    fn sync_from_file(&mut self) {
        if let Ok(content) = std::fs::read_to_string(crate::NOTIF_FILE) {
            if let Ok(file_notifs) = serde_json::from_str::<Vec<Notification>>(&content) {
                let file_panes: std::collections::HashSet<&str> =
                    file_notifs.iter().map(|n| n.pane.as_str()).collect();
                let before = self.notifications.len();
                self.notifications.retain(|n| file_panes.contains(n.pane.as_str()));
                if self.notifications.len() != before {
                    eprintln!("watcher: synced deletions from file");
                }
            }
        }
    }

    fn sync_to_file(&self) {
        if let Ok(json) = serde_json::to_string(&self.notifications) {
            let _ = std::fs::write(crate::NOTIF_FILE, json);
        }
    }

    fn add_notification(&mut self, pane: &str, message: String) {
        if let Some(existing) = self.notifications.iter_mut().find(|n| n.pane == pane) {
            eprintln!("watcher: notification update -> {message}");
            existing.message = message;
        } else {
            eprintln!("watcher: notification -> {message}");
            self.notifications.push(Notification {
                pane: pane.to_string(),
                message,
            });
        }
    }
}

fn poll_status_file(path: &std::path::Path) -> Option<(String, String)> {
    let filename = path.file_name()?.to_str()?;
    let pane = filename.strip_prefix(STATUS_PREFIX)?;
    let content = std::fs::read_to_string(path).ok()?;
    let _ = std::fs::remove_file(path);
    Some((pane.to_string(), content.trim().to_string()))
}

fn watcher_thread(state: SharedState) {
    {
        let mut s = state.lock().unwrap();
        process_existing_files(&mut s);
    }

    let (tx, rx) = std::sync::mpsc::channel();

    let mut watcher =
        match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for path in event.paths {
                        if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(STATUS_PREFIX)) {
                            let _ = tx.send(path);
                        }
                    }
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("watcher: failed to create file watcher: {e}");
                return;
            }
        };

    if let Err(e) = watcher.watch(std::path::Path::new(STATUS_DIR), RecursiveMode::NonRecursive) {
        eprintln!("watcher: failed to watch {STATUS_DIR}: {e}");
        return;
    }

    eprintln!("watcher: monitoring {STATUS_DIR}/{STATUS_PREFIX}*");

    loop {
        {
            let mut s = state.lock().unwrap();
            s.sync_from_file();
        }

        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(path) => {
                if let Some((pane, status)) = poll_status_file(&path) {
                    eprintln!("watcher: {pane} -> {status}");
                    let mut s = state.lock().unwrap();
                    handle_status_change(&pane, &status, &mut s);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let mut s = state.lock().unwrap();
        check_idle_panes(&mut s);
        s.dismiss_visited();
        s.sync_to_file();
    }
}

fn process_existing_files(state: &mut WatcherState) {
    if let Ok(entries) = std::fs::read_dir(STATUS_DIR) {
        for entry in entries.flatten() {
            if entry.file_name().to_str().is_some_and(|n| n.starts_with(STATUS_PREFIX)) {
                if let Some((pane, status)) = poll_status_file(&entry.path()) {
                    handle_status_change(&pane, &status, state);
                }
            }
        }
    }
}

fn handle_status_change(pane: &str, status: &str, state: &mut WatcherState) {
    let prev = state.pane_states.get(pane).map(|s| s.status.clone()).unwrap_or(PaneStatus::Idle);
    let active = crate::tmux::active_windows();
    let is_active = active.iter().any(|a| a == pane);
    let now = Instant::now();

    match status {
        "working" => {
            state.pane_states.insert(pane.to_string(), PaneState { status: PaneStatus::Running, last_activity: now });
        }
        "waiting" => {
            if prev != PaneStatus::Waiting && !is_active {
                state.add_notification(pane, format_notification("Waiting for input", pane));
            }
            state.pane_states.insert(pane.to_string(), PaneState { status: PaneStatus::Waiting, last_activity: now });
        }
        "done" => {
            if prev != PaneStatus::Done && !is_active {
                state.add_notification(pane, format_notification("Finished", pane));
            }
            state.pane_states.insert(pane.to_string(), PaneState { status: PaneStatus::Done, last_activity: now });
        }
        _ => {}
    }
}

fn check_idle_panes(state: &mut WatcherState) {
    let active = crate::tmux::active_windows();
    let now = Instant::now();

    let idle: Vec<String> = state.pane_states.iter()
        .filter(|(_, ps)| ps.status == PaneStatus::Running && now.duration_since(ps.last_activity) >= IDLE_TIMEOUT)
        .map(|(pane, _)| pane.clone())
        .collect();

    for pane in idle {
        eprintln!("watcher: {pane} idle for {}s, marking done", IDLE_TIMEOUT.as_secs());
        if !active.contains(&pane) {
            state.add_notification(&pane, format_notification("Finished", &pane));
        }
        state.pane_states.insert(pane, PaneState { status: PaneStatus::Done, last_activity: now });
    }
}

fn format_notification(status: &str, pane: &str) -> String {
    if let Some((session, index)) = pane.rsplit_once(':') {
        format!("[{index}] {status} in {session}")
    } else {
        format!("{status} in {pane}")
    }
}

// --- X11 overlay ---

fn find_argb_visual(screen: &Screen) -> Option<(Visualid, u8)> {
    for depth_info in &screen.allowed_depths {
        if depth_info.depth == 32 {
            for visual in &depth_info.visuals {
                if visual.class == VisualClass::TRUE_COLOR {
                    return Some((visual.visual_id, 32));
                }
            }
        }
    }
    None
}

fn terminal_window_id() -> Option<Window> {
    std::env::var("WINDOWID").ok()?.parse::<u32>().ok()
}

fn window_geometry(conn: &impl Connection, win: Window) -> Option<(i16, i16, u16, u16)> {
    let geo = conn.get_geometry(win).ok()?.reply().ok()?;
    let trans = conn.translate_coordinates(win, geo.root, 0, 0).ok()?.reply().ok()?;
    Some((trans.dst_x, trans.dst_y, geo.width, geo.height))
}

pub fn run(config: Config) -> Result<()> {
    std::fs::write(crate::PID_FILE, std::process::id().to_string())
        .context("failed to write PID file")?;

    let fallback_x = config.notifications.x as i16;
    let fallback_y = config.notifications.y as i16;
    let width = config.notifications.width;
    let terminal_win = terminal_window_id();
    let state: SharedState = Arc::new(Mutex::new(WatcherState::new()));

    let state_clone = state.clone();
    std::thread::spawn(move || watcher_thread(state_clone));

    let result = run_x11_overlay(state, terminal_win, fallback_x, fallback_y, width);
    let _ = std::fs::remove_file(crate::PID_FILE);
    result
}

fn run_x11_overlay(
    state: SharedState,
    terminal_win: Option<Window>,
    fallback_x: i16,
    fallback_y: i16,
    win_width: u16,
) -> Result<()> {
    let (conn, screen_num) = x11rb::connect(None).context("failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];

    let (visual_id, depth) = find_argb_visual(screen).context("no 32-bit ARGB visual found")?;

    let colormap = conn.generate_id()?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual_id)?;

    let (win_x, win_y) = terminal_win
        .and_then(|tw| window_geometry(&conn, tw))
        .map(|(tx, ty, tw_w, _)| (tx + tw_w as i16 - win_width as i16 - NOTIF_MARGIN, ty + NOTIF_MARGIN))
        .unwrap_or((fallback_x, fallback_y));

    let win = conn.generate_id()?;
    conn.create_window(
        depth, win, screen.root, win_x, win_y, win_width, 1, 0,
        WindowClass::INPUT_OUTPUT, visual_id,
        &CreateWindowAux::new()
            .override_redirect(1)
            .background_pixel(0x00000000)
            .border_pixel(0x00000000)
            .colormap(colormap)
            .event_mask(EventMask::EXPOSURE),
    )?;

    shape::rectangles(&conn, shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &[])?;

    let gc = conn.generate_id()?;
    conn.create_gc(gc, win, &CreateGCAux::new())?;
    conn.flush()?;

    let mut visible = false;
    let mut last_pos = (win_x, win_y);

    loop {
        while let Some(event) = conn.poll_for_event()? {
            if let x11rb::protocol::Event::Expose(_) = event {
                redraw(&conn, win, gc, depth, win_width, &state)?;
            }
        }

        if let Some(tw) = terminal_win {
            if let Some((tx, ty, tw_w, _)) = window_geometry(&conn, tw) {
                let new_x = tx + tw_w as i16 - win_width as i16 - NOTIF_MARGIN;
                let new_y = ty + NOTIF_MARGIN;
                if (new_x, new_y) != last_pos {
                    conn.configure_window(win, &ConfigureWindowAux::new().x(new_x as i32).y(new_y as i32))?;
                    conn.flush()?;
                    last_pos = (new_x, new_y);
                }
            }
        }

        let count = state.lock().unwrap().notifications.len();

        if count > 0 && !visible {
            conn.map_window(win)?;
            conn.flush()?;
            visible = true;
            redraw(&conn, win, gc, depth, win_width, &state)?;
        } else if count == 0 && visible {
            conn.unmap_window(win)?;
            conn.flush()?;
            visible = false;
        } else if visible {
            redraw(&conn, win, gc, depth, win_width, &state)?;
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wrap_text(cr: &cairo::Context, text: &str, max_width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split(' ') {
        let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
        if cr.text_extents(&candidate).unwrap().width() > max_width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
        } else {
            current = candidate;
        }

        while cr.text_extents(&current).unwrap().width() > max_width && current.len() > 1 {
            let mut split = current.len();
            while split > 1 {
                split -= 1;
                if cr.text_extents(&current[..split]).unwrap().width() <= max_width { break; }
            }
            lines.push(current[..split].to_string());
            current = current[split..].to_string();
        }
    }

    if !current.is_empty() { lines.push(current); }
    if lines.is_empty() { lines.push(String::new()); }

    if lines.len() > 2 { lines.truncate(2); }
    if lines.len() == 2 {
        let last = &mut lines[1];
        while cr.text_extents(&format!("{last}...")).unwrap().width() > max_width && last.len() > 1 {
            last.pop();
        }
        *last = format!("{last}...");
    }
    lines
}

fn card_height(line_count: usize) -> f64 {
    CARD_VERT_PADDING + line_count as f64 * LINE_HEIGHT
}

fn compute_total_height(cr: &cairo::Context, notifications: &[Notification], win_width: u16) -> f64 {
    let text_width = win_width as f64 - CARD_MARGIN * 2.0 - CARD_PADDING * 2.0;
    let mut total = CARD_MARGIN;
    for (i, notif) in notifications.iter().enumerate() {
        if i >= MAX_VISIBLE { break; }
        total += card_height(wrap_text(cr, &notif.message, text_width).len()) + CARD_MARGIN;
    }
    total
}

fn redraw(
    conn: &impl Connection,
    win: Window,
    gc: Gcontext,
    depth: u8,
    win_width: u16,
    state: &SharedState,
) -> Result<()> {
    let s = state.lock().unwrap();
    let count = s.notifications.len().min(MAX_VISIBLE);
    if count == 0 { return Ok(()); }

    let width = win_width as i32;

    let measure = ImageSurface::create(Format::ARgb32, 1, 1).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mcr = cairo::Context::new(&measure).map_err(|e| anyhow::anyhow!("{e}"))?;
    mcr.select_font_face("monospace", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    mcr.set_font_size(FONT_SIZE);

    let total_height = compute_total_height(&mcr, &s.notifications, win_width) as i32;
    drop(mcr);

    conn.configure_window(win, &ConfigureWindowAux::new().height(total_height as u32))?;

    let mut surface = ImageSurface::create(Format::ARgb32, width, total_height).map_err(|e| anyhow::anyhow!("{e}"))?;
    let cr = cairo::Context::new(&surface).map_err(|e| anyhow::anyhow!("{e}"))?;

    cr.set_operator(cairo::Operator::Source);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.paint().map_err(|e| anyhow::anyhow!("{e}"))?;
    cr.set_operator(cairo::Operator::Over);

    cr.select_font_face("monospace", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(FONT_SIZE);

    let text_width = width as f64 - CARD_MARGIN * 2.0 - CARD_PADDING * 2.0;
    let mut y = CARD_MARGIN;

    for (i, notif) in s.notifications.iter().enumerate() {
        if i >= MAX_VISIBLE { break; }

        let lines = wrap_text(&cr, &notif.message, text_width);
        let h = card_height(lines.len());

        rounded_rect(&cr, CARD_MARGIN, y, width as f64 - CARD_MARGIN * 2.0, h, CARD_ROUNDING);
        cr.set_source_rgba(0.25, 0.50, 0.85, 0.92);
        cr.fill().map_err(|e| anyhow::anyhow!("{e}"))?;

        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        for (li, line) in lines.iter().enumerate() {
            cr.move_to(CARD_PADDING + CARD_MARGIN, y + CARD_VERT_PADDING * 0.5 + (li as f64 + 1.0) * LINE_HEIGHT - 4.0);
            cr.show_text(line).map_err(|e| anyhow::anyhow!("{e}"))?;
        }

        y += h + CARD_MARGIN;
    }

    drop(cr);
    surface.flush();

    let stride = surface.stride() as usize;
    let data = surface.data().map_err(|e| anyhow::anyhow!("{e}"))?;

    conn.put_image(ImageFormat::Z_PIXMAP, win, gc, width as u16, total_height as u16, 0, 0, 0, depth, &data[..stride * total_height as usize])?;
    conn.flush()?;
    Ok(())
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}
