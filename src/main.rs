mod config;
mod hooks;
mod jump;
mod picker;
mod session;
mod stats;
mod status;
mod tmux;
mod watcher;
mod worktree;

use anyhow::{Context, Result};
use clap::Parser;
use std::process::Command;

#[derive(Parser)]
#[command(name = "hive", about = "Tmux session manager with notifications")]
enum Cli {
    Launch,
    Pick,
    Jump,
    Stats,
    Watch,
    Stop,
    /// Update the agent status for the current tmux pane (used by Claude hooks)
    Status {
        /// Status to record: working, waiting, or done
        state: String,
    },
}

const PID_FILE: &str = "/tmp/hive-watcher.pid";
const NOTIF_FILE: &str = "/tmp/hive-notifications.json";

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli {
        Cli::Stop => return stop(),
        Cli::Status { state } => return status_cmd(state),
        _ => {}
    }

    let config = config::Config::load()?;

    match cli {
        Cli::Launch => launch(config),
        Cli::Pick => picker::run(config),
        Cli::Jump => jump::run(),
        Cli::Stats => stats::run(),
        Cli::Watch => watcher::run(config),
        Cli::Stop | Cli::Status { .. } => unreachable!(),
    }
}

fn status_cmd(state: &str) -> Result<()> {
    if !matches!(state, "working" | "waiting" | "done") {
        anyhow::bail!("invalid status: {state} (expected working|waiting|done)");
    }
    let pane = current_pane().context("could not determine tmux pane")?;
    status::write(&pane, state)?;
    Ok(())
}

fn current_pane() -> Result<String> {
    let mut cmd = Command::new("tmux");
    cmd.arg("display-message").arg("-p");
    if let Ok(pane) = std::env::var("TMUX_PANE") {
        cmd.arg("-t").arg(pane);
    }
    cmd.arg("#S:#I");
    let out = cmd.output().context("running tmux display-message")?;
    if !out.status.success() {
        anyhow::bail!("tmux display-message failed");
    }
    let pane = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if pane.is_empty() {
        anyhow::bail!("tmux returned empty pane id");
    }
    Ok(pane)
}

fn launch(config: config::Config) -> Result<()> {
    let exe = std::env::current_exe().context("could not determine own binary path")?;
    let exe_str = exe.to_string_lossy();

    if let Err(e) = hooks::ensure_installed() {
        eprintln!("hive: could not verify claude hooks: {e:#}");
    }

    Command::new(&exe)
        .arg("watch")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn watcher")?;

    let initial_session = picker::run_and_return(&config)?;

    let Some(name) = initial_session else {
        return Ok(());
    };

    let pick_cmd = format!(
        "display-popup -E -w {}% -h {}% '{}' pick",
        config.picker.width, config.picker.height, exe_str
    );
    let jump_cmd = format!(
        "if [ -s {notif} ] && [ \"$(cat {notif})\" != '[]' ]; then tmux display-popup -E -w {jw}% -h {jh}% '{exe}' jump; fi",
        notif = crate::NOTIF_FILE,
        exe = exe_str,
        jw = config.jump.width,
        jh = config.jump.height,
    );
    let stats_cmd = format!(
        "display-popup -E -w {}% -h {}% '{}' stats",
        config.stats.width, config.stats.height, exe_str
    );

    let status = Command::new("tmux")
        .args([
            "new-session",
            "-A",
            "-s",
            &name,
            ";",
            "bind-key",
            "-n",
            &config.keybindings.picker,
            "run-shell",
            "-b",
            &format!("tmux {pick_cmd}"),
            ";",
            "bind-key",
            "-n",
            &config.keybindings.jump,
            "run-shell",
            "-b",
            &jump_cmd,
            ";",
            "bind-key",
            "-n",
            &config.keybindings.stats,
            "run-shell",
            "-b",
            &format!("tmux {stats_cmd}"),
        ])
        .status()
        .context("failed to start tmux")?;

    if !status.success() {
        anyhow::bail!("tmux exited with {}", status);
    }

    Ok(())
}

fn stop() -> Result<()> {
    let pid_str =
        std::fs::read_to_string(PID_FILE).context("watcher is not running (no PID file found)")?;
    let pid: i32 = pid_str.trim().parse().context("invalid PID file")?;

    let ret = unsafe { libc::kill(pid, libc::SIGTERM) };
    if ret != 0 {
        let _ = std::fs::remove_file(PID_FILE);
        anyhow::bail!("failed to kill watcher (pid {})", pid);
    }

    let _ = std::fs::remove_file(PID_FILE);
    println!("Stopped watcher (pid {})", pid);
    Ok(())
}
