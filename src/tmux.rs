use anyhow::{Context, Result, bail};
use std::process::Command;

fn tmux(args: &[&str]) -> Result<String> {
    let output = Command::new("tmux")
        .args(args)
        .output()
        .context("failed to run tmux")?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn tmux_ok(args: &[&str]) -> Result<()> {
    let status = Command::new("tmux")
        .args(args)
        .status()
        .context("failed to run tmux")?;
    if !status.success() {
        bail!("tmux {:?} failed with {}", args, status);
    }
    Ok(())
}

pub fn has_session(name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", &format!("={name}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn list_sessions() -> Vec<String> {
    tmux(&["list-sessions", "-F", "#S"])
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty() && *l != crate::BOOTSTRAP_SESSION)
        .map(String::from)
        .collect()
}

pub fn current_session() -> Option<String> {
    tmux(&["display-message", "-p", "#S"])
        .ok()
        .filter(|s| !s.is_empty())
}

pub fn kill_session(name: &str) -> Result<()> {
    tmux_ok(&["kill-session", "-t", &format!("={name}")])
}

pub fn switch_client(name: &str) -> Result<()> {
    tmux_ok(&["switch-client", "-t", name])
}

pub fn active_windows() -> Vec<String> {
    // Only count the active window of sessions that actually have a client
    // attached — every session has a window_active=1 window even when no one
    // is looking at it, which would otherwise highlight one tile per session.
    tmux(&[
        "list-windows",
        "-a",
        "-F",
        "#{?session_attached,#{?window_active,#{session_name}:#{window_index},},}",
    ])
    .unwrap_or_default()
    .lines()
    .filter(|l| !l.is_empty())
    .map(String::from)
    .collect()
}

pub fn create_blank_session(name: &str, path: &str) -> Result<()> {
    tmux_ok(&["new-session", "-d", "-s", name, "-c", path])
}

/// Set tmux's global `default-size` to the real controlling-terminal size.
/// Detached sessions inherit this when created, so any splits we perform
/// before a client attaches use accurate cell counts instead of the built-in
/// 80x24 fallback (which would otherwise get proportionally rescaled to
/// gibberish on attach — the source of the "panel is too wide at startup"
/// bug). Best-effort: silently no-ops if we can't read the terminal size.
pub fn sync_default_size_to_terminal() {
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        let size = format!("{cols}x{rows}");
        let _ = tmux_ok(&["set-option", "-g", "default-size", &size]);
    }
}

pub const PANEL_PANE_TITLE: &str = "HIVE_PANEL";

pub fn set_current_pane_title(title: &str) -> Result<()> {
    // Target $TMUX_PANE explicitly: a bare `select-pane -T` tags whatever
    // tmux considers the active pane in the active client, which races with
    // `dedupe_and_normalize_panels` and can cause it to mistake the work
    // pane for the panel and kill the real one.
    if let Ok(pane) = std::env::var("TMUX_PANE") {
        tmux_ok(&["select-pane", "-T", title, "-t", &pane])
    } else {
        tmux_ok(&["select-pane", "-T", title])
    }
}

pub fn list_all_windows() -> Vec<String> {
    tmux(&["list-windows", "-a", "-F", "#{session_name}:#{window_index}"])
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Split a panel pane into the given window. `position` is "right" or "left".
/// Captures the new pane id and tags it with the HIVE_PANEL title synchronously,
/// then dedupes and normalizes width in case a concurrent caller also split.
pub fn split_panel(target: &str, width: u16, position: &str, exe: &str) -> Result<()> {
    let len = width.to_string();
    let cmd = format!("{exe} panel");
    let mut args: Vec<&str> = vec![
        "split-window", "-h", "-d", "-P", "-F", "#{pane_id}", "-l", &len, "-t", target,
    ];
    if position == "left" {
        args.push("-b");
    }
    args.push(&cmd);
    let new_pane_id = tmux(&args)?;
    if new_pane_id.is_empty() {
        return Ok(());
    }
    let _ = tmux_ok(&["select-pane", "-T", PANEL_PANE_TITLE, "-t", &new_pane_id]);
    dedupe_and_normalize_panels(target, width);
    Ok(())
}

/// After a split, keep exactly one HIVE_PANEL (lowest pane id wins) and resize
/// the survivor to the intended width. Running concurrently in multiple
/// processes is safe: the tiebreaker is deterministic so every caller picks
/// the same survivor, and killing an already-gone pane is a harmless no-op.
fn dedupe_and_normalize_panels(target: &str, width: u16) {
    let raw = tmux(&[
        "list-panes",
        "-t",
        target,
        "-F",
        "#{pane_id}\t#{pane_title}",
    ])
    .unwrap_or_default();
    let mut panels: Vec<String> = raw
        .lines()
        .filter_map(|l| {
            let (id, title) = l.split_once('\t')?;
            (title == PANEL_PANE_TITLE).then(|| id.to_string())
        })
        .collect();
    panels.sort_by_key(|id| id.trim_start_matches('%').parse::<u64>().unwrap_or(u64::MAX));
    let Some(keep) = panels.first().cloned() else {
        return;
    };
    for id in panels.iter().skip(1) {
        let _ = tmux_ok(&["kill-pane", "-t", id]);
    }

    // Killing duplicates merges their space into a neighbor, which can leave
    // the survivor wider than intended, so resize it back to the configured cells.
    let _ = tmux_ok(&["resize-pane", "-t", &keep, "-x", &width.to_string()]);
}

fn window_has_panel(target: &str) -> bool {
    tmux(&["list-panes", "-t", target, "-F", "#{pane_title}"])
        .unwrap_or_default()
        .lines()
        .any(|l| l == PANEL_PANE_TITLE)
}

/// Bind a key globally (`bind-key -n`) to `run-shell -b <cmd>`. Re-running with
/// the same key overwrites the previous binding, which is how `hive reload`
/// applies keybinding and popup-size changes without restarting tmux.
pub fn install_global_binding(key: &str, cmd: &str) -> Result<()> {
    tmux_ok(&["bind-key", "-n", key, "run-shell", "-b", cmd])
}

/// Install global tmux hooks so newly created windows/sessions get a panel pane.
/// The hook delegates to `hive add-panel` so the idempotency check (skip windows
/// that already have a panel) lives in Rust rather than tmux format strings.
pub fn install_panel_hook(exe: &str) -> Result<()> {
    let win_hook = format!("run-shell -b \"{exe} add-panel '#{{hook_window}}'\"");
    let sess_hook = format!("run-shell -b \"{exe} add-panel '#{{hook_session}}'\"");
    tmux_ok(&["set-hook", "-g", "after-new-window", &win_hook])?;
    tmux_ok(&["set-hook", "-g", "after-new-session", &sess_hook])?;
    // When a pane exits, if the only pane left in the window is the panel,
    // kill the whole window so the panel doesn't linger as an orphan.
    let prune_hook = format!("run-shell -b \"{exe} prune-panel '#{{hook_window}}'\"");
    tmux_ok(&["set-hook", "-g", "pane-exited", &prune_hook])?;
    // Splits performed against detached sessions inherit tmux's default-size
    // (typically 80x24) and get scaled proportionally on attach, so a 20-cell
    // panel can balloon to 60+ cells once the real client geometry kicks in.
    // Re-apply the configured width every time a client attaches OR is
    // resized — `client-attached` alone has been observed to fire before
    // tmux finishes syncing the new client geometry, leaving the panel at
    // the wrong width on the first attach. `client-resized` fires once the
    // post-attach resize lands, which is the moment we actually want to
    // reapply the configured width.
    let attach_hook = format!("run-shell -b \"{exe} resize-panels\"");
    tmux_ok(&["set-hook", "-g", "client-attached", &attach_hook])?;
    tmux_ok(&["set-hook", "-g", "client-resized", &attach_hook])?;
    Ok(())
}

/// Kill `target` if it has exactly one pane and that pane is the panel.
/// Called from the `pane-exited` hook so closing the last work pane in a
/// window also tears down the panel that was attached to it.
pub fn prune_orphan_panel(target: &str) -> Result<()> {
    let raw = tmux(&[
        "list-panes",
        "-t",
        target,
        "-F",
        "#{pane_id}\t#{pane_title}",
    ])
    .unwrap_or_default();
    let panes: Vec<(&str, &str)> = raw
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .collect();
    if panes.len() == 1 && panes[0].1 == PANEL_PANE_TITLE {
        let _ = tmux_ok(&["kill-window", "-t", target]);
    }
    Ok(())
}

pub fn window_pane_count(target: &str) -> usize {
    tmux(&["list-panes", "-t", target, "-F", "#{pane_id}"])
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .count()
}

/// Add a panel pane to the window if it doesn't already have one. We still
/// skip windows that were already split by the user (pane count > 1 and no
/// existing panel) so we don't invade their custom layout; they can opt in
/// manually via `hive add-panel <target>`. For truly concurrent callers the
/// dedup inside `split_panel` cleans up any races this check lets through.
pub fn ensure_panel_in_window(target: &str, width: u16, position: &str, exe: &str) -> Result<()> {
    if window_has_panel(target) {
        if let Some(id) = find_panel_pane(target) {
            let _ = tmux_ok(&["resize-pane", "-t", &id, "-x", &width.to_string()]);
        }
        return Ok(());
    }
    if window_pane_count(target) != 1 {
        return Ok(());
    }
    split_panel(target, width, position, exe)
}

fn find_panel_pane(target: &str) -> Option<String> {
    tmux(&[
        "list-panes",
        "-t",
        target,
        "-F",
        "#{pane_id}\t#{pane_title}",
    ])
    .ok()?
    .lines()
    .find_map(|l| {
        let (id, title) = l.split_once('\t')?;
        (title == PANEL_PANE_TITLE).then(|| id.to_string())
    })
}

/// Toggle focus between the panel pane and the previously focused pane.
/// If the current pane already is the panel, jump back via `last-pane`;
/// otherwise find the panel pane in the current window and select it.
pub fn focus_panel_toggle() -> Result<()> {
    let current_title = tmux(&["display-message", "-p", "#{pane_title}"]).unwrap_or_default();
    if current_title == PANEL_PANE_TITLE {
        return tmux_ok(&["select-pane", "-l"]);
    }
    let window = tmux(&["display-message", "-p", "#{session_name}:#{window_index}"])
        .unwrap_or_default();
    if window.is_empty() {
        return Ok(());
    }
    let raw = tmux(&[
        "list-panes",
        "-t",
        &window,
        "-F",
        "#{pane_id}\t#{pane_title}",
    ])
    .unwrap_or_default();
    let panel_id = raw.lines().find_map(|l| {
        let (id, title) = l.split_once('\t')?;
        (title == PANEL_PANE_TITLE).then(|| id.to_string())
    });
    if let Some(id) = panel_id {
        tmux_ok(&["select-pane", "-t", &id])?;
    }
    Ok(())
}

pub fn ensure_panels_in_all_windows(width: u16, position: &str, exe: &str) {
    for window in list_all_windows() {
        let _ = ensure_panel_in_window(&window, width, position, exe);
    }
}

pub fn switch_session_relative(delta: i32) -> Result<Option<String>> {
    let current = tmux(&["display-message", "-p", "#S"])?;
    if current.is_empty() {
        bail!("could not determine current session");
    }
    let sessions = list_sessions();
    if sessions.len() <= 1 {
        return Ok(None);
    }
    let cur_idx = sessions
        .iter()
        .position(|s| s == &current)
        .unwrap_or(0) as i32;
    let len = sessions.len() as i32;
    let next = ((cur_idx + delta).rem_euclid(len)) as usize;
    let target = &sessions[next];
    switch_client(target)?;
    Ok(Some(target.clone()))
}

pub fn close_current_session() -> Result<Option<String>> {
    let current = tmux(&["display-message", "-p", "#S"])?;
    if current.is_empty() {
        bail!("could not determine current session");
    }
    let sessions = list_sessions();
    let others: Vec<&String> = sessions.iter().filter(|s| **s != current).collect();
    if others.is_empty() {
        kill_session(&current)?;
        return Ok(None);
    }
    let cur_idx = sessions.iter().position(|s| s == &current);
    let target = match cur_idx {
        Some(i) if i > 0 => &sessions[i - 1],
        _ => others[0],
    };
    let target = target.clone();
    let client = tmux(&["display-message", "-p", "#{client_name}"])?;
    if client.is_empty() {
        switch_client(&target)?;
    } else {
        tmux_ok(&["switch-client", "-c", &client, "-t", &target])?;
    }
    kill_session(&current)?;
    Ok(Some(target))
}

pub fn create_project_session(
    name: &str,
    path: &str,
    cmd: &str,
    panel: Option<(u16, &str, &str)>,
) -> Result<()> {
    let prefix = &name[..1];
    let run_win = format!("{prefix}-run");
    let claude_win = format!("{prefix}-claude");
    let sh_win = format!("{prefix}-sh");

    tmux_ok(&["new-session", "-d", "-s", name, "-n", &run_win, "-c", path])?;
    tmux_ok(&["send-keys", "-t", &format!("{name}:{run_win}"), cmd, "C-m"])?;

    // `-d` keeps the active window pinned to window 0 (the run window) while
    // we add the rest. Without it, each new-window switches the session's
    // active window, which makes the backgrounded `after-new-session` panel
    // hook (whose target is just the session name) end up resolving to the
    // wrong window — leaving window 0 without a HIVE_PANEL until a later
    // sweep splits one in under detached-session geometry.
    tmux_ok(&["new-window", "-d", "-t", name, "-n", &claude_win, "-c", path])?;
    tmux_ok(&[
        "send-keys",
        "-t",
        &format!("{name}:{claude_win}"),
        "claude",
        "C-m",
    ])?;

    // Pre-register the claude window as idle so the dock shows it immediately —
    // we can't wait for Claude's SessionStart hook to fire (which only happens
    // once Claude itself has fully booted, and only if hooks are installed).
    let claude_target = format!("{name}:{claude_win}");
    if let Ok(pane) = tmux(&[
        "display-message",
        "-t",
        &claude_target,
        "-p",
        "#S:#I",
    ]) {
        let _ = crate::status::write(&pane, "idle");
    }

    tmux_ok(&["new-window", "-d", "-t", name, "-n", &sh_win, "-c", path])?;
    tmux_ok(&[
        "send-keys",
        "-t",
        &format!("{name}:{sh_win}"),
        "git status",
        "C-m",
    ])?;

    // Now that all windows exist, jump back to the run window so the user
    // lands there on attach (instead of whichever window happened to be
    // created last).
    let _ = tmux_ok(&["select-window", "-t", &format!("{name}:{run_win}")]);

    // Synchronously install panels in every window we just created instead
    // of relying on the async `after-new-window` / `after-new-session` hooks
    // — those used to fire `run-shell -b hive add-panel` per window in
    // background, and the resulting concurrent processes raced badly enough
    // that some windows ended up panel-less. Doing this in-process here makes
    // panel coverage deterministic for hive-managed sessions; the global
    // hooks remain installed so windows the user creates manually later
    // still get covered.
    if let Some((width, position, exe)) = panel {
        for win in [&run_win, &claude_win, &sh_win] {
            let target = format!("{name}:{win}");
            let _ = ensure_panel_in_window(&target, width, position, exe);
        }
    }

    Ok(())
}
