use crate::config::Config;
use crate::voice::intent::Intent;
use anyhow::{Context, Result};
use std::process::Command;

pub fn dispatch(intent: Intent, config: &Config, dictation_target: Option<&str>) -> Result<()> {
    let exe = std::env::current_exe()
        .context("could not determine own binary path")?
        .to_string_lossy()
        .into_owned();

    match intent {
        Intent::OpenPicker => popup(&exe, "pick", config.picker.width, config.picker.height, None),
        Intent::OpenDock => open_dock(&exe, config),
        Intent::OpenStats => popup(&exe, "stats", config.stats.width, config.stats.height, None),
        Intent::OpenNotifications => {
            popup(&exe, "jump", config.jump.width, config.jump.height, None)
        }
        Intent::JumpTo(session) => jump_to_session(&session),
        Intent::JumpToIndex(n) => jump_to_index(n),
        Intent::NextAgent => jump_relative(1),
        Intent::PreviousAgent => jump_relative(-1),
        Intent::NextSession => switch_session_and_focus(1),
        Intent::PreviousSession => switch_session_and_focus(-1),
        Intent::Btw(text) => send_btw(dictation_target, config.voice.vim_mode, text.as_deref()),
        Intent::Accept => send_accept(config.voice.vim_mode),
        Intent::Choose(n) => send_choose(n, config.voice.vim_mode),
        // Leading space so consecutive dictations don't run together; the
        // prompt input trims leading whitespace on submit.
        Intent::Dictate(text) => send_to_dictation(
            dictation_target,
            config.voice.vim_mode,
            &["-l", &format!(" {text}")],
        ),
        Intent::Send => send_to_dictation(dictation_target, config.voice.vim_mode, &["Enter"]),
        Intent::Cancel | Intent::Clear => {
            send_to_dictation(dictation_target, config.voice.vim_mode, &["C-u"])
        }
        Intent::CloseSession(None) => {
            if let Some(target) = crate::tmux::close_current_session()? {
                focus_agent_window(&target);
            }
            Ok(())
        }
        Intent::CloseSession(Some(name)) => crate::tmux::kill_session(&name),
        Intent::OpenSession(name) => open_session(&name, config),
    }
}

fn switch_session_and_focus(delta: i32) -> Result<()> {
    if let Some(target) = crate::tmux::switch_session_relative(delta)? {
        focus_agent_window(&target);
    }
    Ok(())
}

fn open_session(name: &str, config: &Config) -> Result<()> {
    if !crate::tmux::has_session(name) {
        if let Some(path) = crate::session::resolve_path(name, config) {
            let cmd = crate::session::resolve_cmd(name, config);
            let panel = if config.panel.enabled {
                let exe = std::env::current_exe()
                    .ok()
                    .map(|e| e.to_string_lossy().into_owned());
                exe.map(|e| (config.panel.width, config.panel.position.clone(), e))
            } else {
                None
            };
            crate::tmux::create_project_session(
                name,
                &path.to_string_lossy(),
                &cmd,
                panel.as_ref().map(|(w, p, e)| (*w, p.as_str(), e.as_str())),
            )?;
        } else {
            let home = dirs::home_dir()
                .map(|h| h.to_string_lossy().into_owned())
                .unwrap_or_else(|| "~".to_string());
            crate::tmux::create_blank_session(name, &home)?;
        }
    }
    crate::tmux::switch_client(name)?;
    focus_agent_window(name);
    Ok(())
}

fn open_dock(exe: &str, config: &Config) -> Result<()> {
    if config.panel.enabled {
        return tmux(&["run-shell", "-b", &format!("{exe} focus-panel")]);
    }
    popup(
        exe,
        "dock",
        config.dock.width,
        config.dock.height,
        Some(&config.dock.position),
    )
}

fn popup(exe: &str, sub: &str, w: u8, h: u8, pos: Option<&str>) -> Result<()> {
    let w_arg = format!("{w}%");
    let h_arg = format!("{h}%");
    let mut args: Vec<String> = vec![
        "display-popup".into(),
        "-E".into(),
        "-w".into(),
        w_arg,
        "-h".into(),
        h_arg,
    ];
    match pos {
        Some("left") => {
            args.push("-x".into());
            args.push("0".into());
        }
        Some("right") => {
            args.push("-x".into());
            args.push("R".into());
        }
        _ => {}
    }
    args.push(exe.to_string());
    args.push(sub.to_string());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tmux(&arg_refs)
}

fn jump_to_session(session: &str) -> Result<()> {
    crate::tmux::switch_client(session)?;
    focus_agent_window(session);
    Ok(())
}

fn jump_to_index(n: usize) -> Result<()> {
    let panes = crate::dock::ordered_panes();
    let idx = n.checked_sub(1).context("agent index must be >= 1")?;
    let pane = panes
        .get(idx)
        .with_context(|| format!("no agent at index {n} (only {} live)", panes.len()))?;
    jump_to_pane(pane)
}

fn jump_relative(delta: i32) -> Result<()> {
    let panes = crate::dock::ordered_panes();
    if panes.is_empty() {
        anyhow::bail!("no agents to navigate");
    }
    let cur = focused_pane_target()
        .as_deref()
        .and_then(|c| panes.iter().position(|p| p == c))
        .map(|i| i as i32)
        .unwrap_or(-1);
    let len = panes.len() as i32;
    let next = ((cur + delta).rem_euclid(len)) as usize;
    jump_to_pane(&panes[next])
}

fn jump_to_pane(pane: &str) -> Result<()> {
    let (session, window) = pane
        .rsplit_once(':')
        .with_context(|| format!("malformed pane id {pane}"))?;
    crate::tmux::switch_client(session)?;
    let _ = tmux(&["select-window", "-t", &format!("{session}:{window}")]);
    Ok(())
}

fn focus_agent_window(session: &str) {
    if let Some(window) = find_agent_window(session) {
        let _ = tmux(&["select-window", "-t", &window]);
    }
}

fn find_agent_window(session: &str) -> Option<String> {
    for path in crate::status::list() {
        let entry = crate::status::read(&path)?;
        if let Some((s, _)) = entry.pane.split_once(':') {
            if s == session {
                return Some(entry.pane);
            }
        }
    }
    let prefix = session.chars().next()?;
    Some(format!("{session}:{prefix}-claude"))
}

fn send_to_dictation(captured: Option<&str>, vim_mode: bool, keys: &[&str]) -> Result<()> {
    let target = captured
        .map(str::to_string)
        .or_else(focused_pane_target)
        .context("could not determine target tmux pane")?;
    ensure_insert_mode(&target, vim_mode)?;
    let mut args: Vec<&str> = vec!["send-keys", "-t", &target];
    args.extend_from_slice(keys);
    tmux(&args)
}

fn send_btw(captured: Option<&str>, vim_mode: bool, text: Option<&str>) -> Result<()> {
    match text {
        Some(t) => {
            send_to_dictation(captured, vim_mode, &["-l", &format!("/btw {t}")])?;
            send_to_dictation(captured, vim_mode, &["Enter"])
        }
        None => send_to_dictation(captured, vim_mode, &["-l", "/btw"]),
    }
}

fn send_choose(n: usize, vim_mode: bool) -> Result<()> {
    let target = most_recent_waiting()
        .or_else(focused_pane_target)
        .context("no waiting pane and no focused pane")?;
    ensure_insert_mode(&target, vim_mode)?;
    tmux(&["send-keys", "-t", &target, &n.to_string()])
}

fn send_accept(vim_mode: bool) -> Result<()> {
    let target = most_recent_waiting()
        .or_else(focused_pane_target)
        .context("no waiting pane and no focused pane to accept on")?;
    ensure_insert_mode(&target, vim_mode)?;
    tmux(&["send-keys", "-t", &target, "1", "Enter"])
}

fn most_recent_waiting() -> Option<String> {
    let mut latest: Option<(i64, String)> = None;
    for path in crate::status::list() {
        let Some(entry) = crate::status::read(&path) else {
            continue;
        };
        if entry.status != "waiting" {
            continue;
        }
        if latest.as_ref().map_or(true, |(ts, _)| entry.ts > *ts) {
            latest = Some((entry.ts, entry.pane));
        }
    }
    latest.map(|(_, pane)| pane)
}

fn focused_pane_target() -> Option<String> {
    let out = Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}:#{window_index}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

// Escape+i is idempotent across vim sub-modes — Escape always lands in
// normal, then `i` re-enters insert. Safe to send unconditionally.
fn ensure_insert_mode(target: &str, vim_mode: bool) -> Result<()> {
    if !vim_mode {
        return Ok(());
    }
    tmux(&["send-keys", "-t", target, "Escape", "i"])
}

fn tmux(args: &[&str]) -> Result<()> {
    let status = Command::new("tmux")
        .args(args)
        .status()
        .with_context(|| format!("running tmux {args:?}"))?;
    if !status.success() {
        anyhow::bail!("tmux {args:?} exited with {status}");
    }
    Ok(())
}
