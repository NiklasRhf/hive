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
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

pub fn kill_session(name: &str) -> Result<()> {
    tmux_ok(&["kill-session", "-t", &format!("={name}")])
}

pub fn switch_client(name: &str) -> Result<()> {
    tmux_ok(&["switch-client", "-t", name])
}

pub fn active_windows() -> Vec<String> {
    tmux(&["list-clients", "-F", "#{session_name}:#{window_index}"])
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

pub fn create_project_session(name: &str, path: &str, cmd: &str) -> Result<()> {
    let prefix = &name[..1];
    let run_win = format!("{prefix}-run");
    let claude_win = format!("{prefix}-claude");
    let sh_win = format!("{prefix}-sh");

    tmux_ok(&["new-session", "-d", "-s", name, "-n", &run_win, "-c", path])?;
    tmux_ok(&["send-keys", "-t", &format!("{name}:{run_win}"), cmd, "C-m"])?;

    tmux_ok(&["new-window", "-t", name, "-n", &claude_win, "-c", path])?;
    tmux_ok(&[
        "send-keys",
        "-t",
        &format!("{name}:{claude_win}"),
        "claude",
        "C-m",
    ])?;

    tmux_ok(&["new-window", "-t", name, "-n", &sh_win, "-c", path])?;
    tmux_ok(&[
        "send-keys",
        "-t",
        &format!("{name}:{sh_win}"),
        "git status",
        "C-m",
    ])?;

    Ok(())
}
